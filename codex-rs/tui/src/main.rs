use clap::Parser;
use codex_arg0::Arg0DispatchPaths;
use codex_arg0::arg0_dispatch_or_else;
use codex_tui::Cli;
use codex_tui::run_main;
use codex_utils_cli::CliConfigOverrides;

#[derive(Parser, Debug)]
struct TopCli {
    #[clap(flatten)]
    config_overrides: CliConfigOverrides,

    #[clap(flatten)]
    inner: Cli,
}

fn main() -> anyhow::Result<()> {
    arg0_dispatch_or_else(|arg0_paths: Arg0DispatchPaths| async move {
        let top_cli = TopCli::parse();
        let mut inner = top_cli.inner;
        inner
            .config_overrides
            .raw_overrides
            .splice(0..0, top_cli.config_overrides.raw_overrides);
        if inner.config_overrides.settings_file.is_none() {
            inner.config_overrides.settings_file = top_cli.config_overrides.settings_file;
        }
        let exit_info = run_main(
            inner,
            arg0_paths,
            codex_core::config_loader::LoaderOverrides::default(),
            /*remote*/ None,
            /*remote_auth_token*/ None,
        )
        .await?;
        let token_usage = exit_info.token_usage;
        if !token_usage.is_zero() {
            println!(
                "{}",
                codex_protocol::protocol::FinalOutput::from(token_usage),
            );
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn top_cli_keeps_root_level_settings_file_for_inner_cli() {
        let cli = TopCli::parse_from(["codex", "--settings", "/tmp/settings.json", "hello"]);

        let mut inner = cli.inner;
        if inner.config_overrides.settings_file.is_none() {
            inner.config_overrides.settings_file = cli.config_overrides.settings_file;
        }

        assert_eq!(
            inner.config_overrides.settings_file.as_deref(),
            Some(std::path::Path::new("/tmp/settings.json"))
        );
    }
}
