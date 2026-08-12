use crate::config::Config;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_analytics::InvocationType;
use codex_analytics::SkillInvocation;
use codex_analytics::SkillInvocationLocation;
use codex_analytics::TrackEventsContext;
use codex_analytics::build_track_events_context;
use codex_extension_api::SkillInvocationInput;
use codex_extension_api::SkillInvocationKind;
use codex_otel::sanitize_metric_tag_value;
use codex_protocol::protocol::SkillScope;
use codex_skills_extension::detect_implicit_skill_invocation;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
use codex_utils_plugins::PluginSkillRoot;
use std::collections::HashSet;
use tokio::sync::Mutex;

pub use codex_skills::SkillError;
pub use codex_skills::SkillMetadata;
pub use codex_skills::SkillPolicy;
pub use codex_skills::build_skill_name_counts;
pub use codex_skills::collect_explicit_skill_mentions;
pub use codex_skills::detect_implicit_skill_invocation_for_command;
pub use codex_skills_extension::HostSkillsLoadInput;
pub use codex_skills_extension::HostSkillsService;
pub use codex_skills_extension::SkillLoadOutcome;
pub use codex_skills_extension::bundled_skills_enabled_from_stack;
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use tracing::warn;

pub use crate::skill_env_var_dependencies::SkillDependencyInfo;
pub use crate::skill_env_var_dependencies::collect_env_var_dependencies;

#[derive(Debug, Default)]
struct ImplicitSkillInvocations(Mutex<HashSet<String>>);

pub(crate) fn skills_load_input_from_config(
    config: &Config,
    effective_skill_roots: Vec<PluginSkillRoot>,
) -> HostSkillsLoadInput {
    HostSkillsLoadInput::new(
        config.cwd.clone(),
        effective_skill_roots,
        config.config_layer_stack.clone(),
    )
}

pub(crate) fn emit_explicit_skill_invocations(
    sess: &Session,
    turn_context: &TurnContext,
    mentioned_skills: &[SkillMetadata],
    injected_skills: &[SkillMetadata],
    tracking: TrackEventsContext,
) {
    let injected_skill_paths = injected_skills
        .iter()
        .map(|skill| &skill.path_to_skills_md)
        .collect::<HashSet<_>>();
    for skill in mentioned_skills {
        let skill_name_tag = sanitize_metric_tag_value(skill.name.as_str());
        let status = if injected_skill_paths.contains(&skill.path_to_skills_md) {
            "ok"
        } else {
            "error"
        };
        turn_context.session_telemetry.counter(
            "codex.skill.injected",
            /*inc*/ 1,
            &[
                ("status", status),
                ("skill", skill_name_tag.as_str()),
                ("invoke_type", "explicit"),
            ],
        );
    }

    let invocations = injected_skills
        .iter()
        .map(|skill| SkillInvocation {
            skill_name: skill.name.clone(),
            location: SkillInvocationLocation::Host {
                path: skill.path_to_skills_md.to_path_buf(),
                scope: skill.scope,
            },
            plugin_id: skill.plugin_id.clone(),
            remote_plugin_id: skill.remote_plugin_id.clone(),
            invocation_type: InvocationType::Explicit,
        })
        .collect();
    sess.services
        .analytics_events_client
        .track_skill_invocations(tracking, invocations);
}

pub(crate) fn include_cli_plugin_skill_roots(
    config: &Config,
    mut effective_skill_roots: Vec<PluginSkillRoot>,
) -> Vec<PluginSkillRoot> {
    for cli_plugin_dir in &config.cli_plugin_dirs {
        if let Some(root) = codex_utils_plugins::plugin_skill_root_from_cli_dir(cli_plugin_dir) {
            effective_skill_roots.push(root);
        } else {
            warn!(
                "--plugin-dir {} skipped: no readable plugin manifest at .codex-plugin/plugin.json or .claude-plugin/plugin.json",
                cli_plugin_dir.display()
            );
        }
    }
    effective_skill_roots
}

/// Extend `effective` with [`PluginHookSource`] entries discovered under each
/// directory passed via `--plugin-dir`. Mirrors [`include_cli_plugin_skill_roots`]
/// for the hooks subsystem: PR #57 added the skill-root path; this completes the
/// parity so CLI-injected plugins can register hooks alongside skills + agents.
///
/// Without this, plugins shipped via `--plugin-dir` are silent on `PreToolUse` /
/// `PostToolUse` etc., even though their `hooks/hooks.json` sidecar exists and
/// the plugin-hook loader (`load_plugin_hooks`) is implemented for marketplace
/// plugins.
pub(crate) fn include_cli_plugin_hook_sources(
    config: &Config,
    mut effective: Vec<codex_plugin::PluginHookSource>,
) -> Vec<codex_plugin::PluginHookSource> {
    for cli_plugin_dir in &config.cli_plugin_dirs {
        let canonical = match std::fs::canonicalize(cli_plugin_dir) {
            Ok(path) => path,
            Err(err) => {
                warn!(
                    "--plugin-dir {}: cannot canonicalize for hook discovery: {err}",
                    cli_plugin_dir.display()
                );
                continue;
            }
        };
        let abs_root = match AbsolutePathBuf::from_absolute_path_checked(canonical) {
            Ok(abs) => abs,
            Err(_) => continue,
        };
        let Some(manifest) = codex_core_plugins::manifest::load_plugin_manifest(abs_root.as_path()) else {
            // Already warned by include_cli_plugin_skill_roots for the same dir.
            continue;
        };
        // CLI-injected plugins don't come from a marketplace; synthesize a
        // marketplace segment ("cli") so the resulting PluginId is well-formed
        // and self-documenting in telemetry/source paths.
        let plugin_id = match codex_plugin::PluginId::new(manifest.name.clone(), "cli".to_string()) {
            Ok(id) => id,
            Err(err) => {
                warn!(
                    "--plugin-dir {} skipped for hooks: invalid plugin name {:?}: {err:?}",
                    cli_plugin_dir.display(),
                    manifest.name,
                );
                continue;
            }
        };
        // CLI-injected plugins do not have a separate data root; reuse the
        // plugin root itself (marketplace plugins get a per-plugin cache dir,
        // but CLI-injected ones write to their own tree if needed).
        let plugin_data_root = abs_root.clone();
        let (sources, warnings) =
            codex_core_plugins::loader::load_plugin_hooks(&abs_root, &plugin_id, &plugin_data_root, &manifest.paths);
        for warning in warnings {
            warn!(
                "--plugin-dir {} hook load warning: {warning}",
                cli_plugin_dir.display()
            );
        }
        effective.extend(sources);
    }
    effective
}

pub(crate) async fn resolve_skill_dependencies_for_turn(
    sess: &Arc<Session>,
    turn_context: &TurnContext,
    dependencies: &[crate::skill_env_var_dependencies::SkillDependencyInfo],
) {
    if dependencies.is_empty() {
        return;
    }

    let existing_env = sess.dependency_env().await;
    let mut loaded_values = HashMap::new();
    let mut missing = Vec::new();
    let mut seen_names = HashSet::new();

    for dependency in dependencies {
        let name = dependency.name.clone();
        if !seen_names.insert(name.clone()) || existing_env.contains_key(&name) {
            continue;
        }
        match env::var(&name) {
            Ok(value) => {
                loaded_values.insert(name.clone(), value);
            }
            Err(env::VarError::NotPresent) => {
                missing.push(dependency.clone());
            }
            Err(err) => {
                warn!("failed to read env var {name}: {err}");
                missing.push(dependency.clone());
            }
        }
    }

    if !loaded_values.is_empty() {
        sess.set_dependency_env(loaded_values).await;
    }

    if !missing.is_empty() {
        request_skill_dependencies(sess, turn_context, &missing).await;
    }
}

async fn request_skill_dependencies(
    sess: &Arc<Session>,
    turn_context: &TurnContext,
    dependencies: &[crate::skill_env_var_dependencies::SkillDependencyInfo],
) {
    let questions = dependencies
        .iter()
        .map(|dependency| {
            let requirement = dependency.description.as_ref().map_or_else(
                || {
                    format!(
                        "The skill \"{}\" requires \"{}\" to be set.",
                        dependency.skill_name, dependency.name
                    )
                },
                |description| {
                    format!(
                        "The skill \"{}\" requires \"{}\" to be set ({}).",
                        dependency.skill_name, dependency.name, description
                    )
                },
            );
            codex_protocol::request_user_input::RequestUserInputQuestion {
                id: dependency.name.clone(),
                header: "Skill requires environment variable".to_string(),
                question: format!(
                    "{requirement} This is an experimental internal feature. The value is stored in memory for this session only."
                ),
                is_other: false,
                is_secret: true,
                options: None,
            }
        })
        .collect::<Vec<_>>();
    if questions.is_empty() {
        return;
    }

    let response = sess
        .request_user_input(
            turn_context,
            format!("skill-deps-{}", turn_context.sub_id),
            codex_protocol::request_user_input::RequestUserInputArgs {
                questions,
                is_blocking: true,
                auto_resolution_ms: None,
            },
        )
        .await
        .unwrap_or_else(|| codex_protocol::request_user_input::RequestUserInputResponse {
            answers: HashMap::new(),
        });
    if response.answers.is_empty() {
        return;
    }

    let mut values = HashMap::new();
    for (name, answer) in response.answers {
        let mut user_note = None;
        for entry in &answer.answers {
            if let Some(note) = entry.strip_prefix("user_note: ")
                && !note.trim().is_empty()
            {
                user_note = Some(note.trim().to_string());
            }
        }
        if let Some(value) = user_note {
            values.insert(name, value);
        }
    }
    if values.is_empty() {
        return;
    }

    sess.set_dependency_env(values).await;
}


pub(crate) async fn maybe_emit_implicit_skill_invocation(
    sess: &Session,
    turn_context: &TurnContext,
    command: &str,
    workdir: &PathUri,
    native_workdir: Option<&AbsolutePathBuf>,
    environment_id: &str,
) {
    let Some(invocation) = detect_implicit_skill_invocation(
        turn_context.extension_data.as_ref(),
        environment_id,
        command,
        workdir,
        native_workdir,
    ) else {
        return;
    };
    let skill_name = invocation.skill_name.clone();
    let (skill_resource, seen_key) = match &invocation.location {
        SkillInvocationLocation::Host { path, scope } => {
            let skill_scope = match scope {
                SkillScope::User => "user",
                SkillScope::Repo => "repo",
                SkillScope::System => "system",
                SkillScope::Admin => "admin",
            };
            let skill_path = path.to_string_lossy().into_owned();
            let seen_key = format!("{skill_scope}:{skill_path}:{skill_name}");
            (skill_path, seen_key)
        }
        SkillInvocationLocation::Resource { id, .. } => (id.clone(), format!("resource:{id}")),
    };
    let inserted = {
        let skill_invocations = turn_context
            .extension_data
            .get_or_init(ImplicitSkillInvocations::default);
        let mut seen_skills = skill_invocations.0.lock().await;
        seen_skills.insert(seen_key)
    };
    if !inserted {
        return;
    }
    let skill_name_tag = sanitize_metric_tag_value(skill_name.as_str());

    for contributor in sess.services.extensions.skill_invocation_contributors() {
        contributor
            .on_skill_invocation(SkillInvocationInput {
                session_store: &sess.services.session_extension_data,
                thread_store: &sess.services.thread_extension_data,
                turn_store: turn_context.extension_data.as_ref(),
                turn_id: turn_context.sub_id.as_str(),
                skill_resource: skill_resource.as_str(),
                kind: SkillInvocationKind::Implicit,
            })
            .await;
    }

    turn_context.session_telemetry.counter(
        "codex.skill.injected",
        /*inc*/ 1,
        &[
            ("status", "ok"),
            ("skill", skill_name_tag.as_str()),
            ("invoke_type", "implicit"),
        ],
    );
    sess.services
        .analytics_events_client
        .track_skill_invocations(
            build_track_events_context(
                turn_context.model_info.slug.clone(),
                sess.thread_id.to_string(),
                turn_context.sub_id.clone(),
                turn_context.originator.clone(),
            ),
            vec![invocation],
        );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn include_cli_plugin_skill_roots_adds_roots_from_config() {
        let tmp = tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("plugins/kookr");
        fs::create_dir_all(plugin_root.join(".codex-plugin")).expect("mkdir manifest");
        fs::create_dir_all(plugin_root.join("skills")).expect("mkdir skills");
        fs::write(
            plugin_root.join(".codex-plugin/plugin.json"),
            r#"{"name":"kookr-toolkit"}"#,
        )
        .expect("write manifest");

        let mut config = crate::config::test_config().await;
        config.cli_plugin_dirs = vec![plugin_root.clone()];
        let canonical_plugin_root = fs::canonicalize(plugin_root).expect("canonical plugin root");
        let expected_plugin_root =
            AbsolutePathBuf::from_absolute_path_checked(canonical_plugin_root)
                .expect("absolute plugin root");
        let expected_skill_root = expected_plugin_root.join("skills");

        assert_eq!(
            include_cli_plugin_skill_roots(&config, Vec::new()),
            vec![PluginSkillRoot {
                path: expected_skill_root,
                plugin_identity: PluginIdentity {
                    plugin_id: "kookr-toolkit".to_string(),
                    remote_plugin_id: None,
                },
                plugin_namespace: "kookr-toolkit".to_string(),
                plugin_root: expected_plugin_root,
                discovery_mode: SkillDiscoveryMode::Recursive,
            }]
        );
    }
}
