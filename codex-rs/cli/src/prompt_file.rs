use codex_tui::Cli as TuiCli;

/// Upper bound on `--prompt-file` size. The positional `PROMPT` argument is
/// implicitly capped by `ARG_MAX` (~2 MiB usable); `--prompt-file` removes that
/// ceiling, so a hard limit is reintroduced here to keep an accidental log or
/// binary file from allocating unbounded memory at startup and injecting a
/// runaway initial context item. 10 MiB is far above any real task prompt
/// (which cannot usefully exceed the model context window) yet well below
/// "pointed at the wrong file" territory.
const MAX_PROMPT_FILE_BYTES: u64 = 10 * 1024 * 1024;

/// Fold `--prompt-file` into the positional `prompt` slot. After this runs the
/// file-sourced prompt is indistinguishable from a positional CLI prompt: it
/// sets `skip_update_prompt` and is submitted internally via
/// `create_initial_user_message`, so an orchestrator launching Codex in a PTY
/// can deliver the initial prompt without a terminal-input race.
///
/// `--prompt-file` and a positional `PROMPT` are mutually exclusive at the clap
/// layer; the `prompt.is_some()` guard only matters if the two ever arrive
/// through separate arg matchers (base CLI vs. a flattened subcommand).
///
/// The file size is checked against [`MAX_PROMPT_FILE_BYTES`] before it is read,
/// so an oversized file is rejected rather than read into memory.
pub(crate) fn resolve_prompt_file(interactive: &mut TuiCli) -> std::io::Result<()> {
    if interactive.prompt.is_some() {
        return Ok(());
    }
    if let Some(path) = interactive.prompt_file.take() {
        let read_err = |e: std::io::Error| {
            std::io::Error::new(
                e.kind(),
                format!("failed to read --prompt-file {}: {e}", path.display()),
            )
        };
        let len = std::fs::metadata(&path).map_err(&read_err)?.len();
        if len > MAX_PROMPT_FILE_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "--prompt-file {} is {len} bytes, over the {MAX_PROMPT_FILE_BYTES}-byte limit",
                    path.display(),
                ),
            ));
        }
        let text = std::fs::read_to_string(&path).map_err(&read_err)?;
        interactive.prompt = Some(text);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn reads_file_into_prompt() {
        let path = std::env::temp_dir().join(format!(
            "codex-prompt-file-resolve-{}.txt",
            std::process::id()
        ));
        std::fs::write(&path, "do the thing").expect("write temp prompt");
        let mut cli =
            TuiCli::try_parse_from(["codex", "--prompt-file", path.to_str().expect("utf8 path")])
                .expect("parse should succeed");

        resolve_prompt_file(&mut cli).expect("resolve should succeed");

        assert_eq!(cli.prompt.as_deref(), Some("do the thing"));
        assert_eq!(cli.prompt_file, None);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_is_error() {
        let mut cli = TuiCli::try_parse_from([
            "codex",
            "--prompt-file",
            "/nonexistent/codex-prompt-file/zzz.txt",
        ])
        .expect("parse should succeed");

        let err = resolve_prompt_file(&mut cli).expect_err("missing file must error");
        assert!(
            err.to_string().contains("--prompt-file"),
            "error should name the flag: {err}"
        );
    }

    #[test]
    fn rejects_oversize_file() {
        let path = std::env::temp_dir().join(format!(
            "codex-prompt-file-oversize-{}.txt",
            std::process::id()
        ));
        std::fs::write(&path, vec![b'x'; MAX_PROMPT_FILE_BYTES as usize + 1])
            .expect("write oversize temp prompt");
        let mut cli =
            TuiCli::try_parse_from(["codex", "--prompt-file", path.to_str().expect("utf8 path")])
                .expect("parse should succeed");

        let err = resolve_prompt_file(&mut cli).expect_err("oversize file must error");
        let _ = std::fs::remove_file(&path);
        assert!(
            err.to_string().contains("limit"),
            "error should mention the size limit: {err}"
        );
        assert_eq!(
            cli.prompt, None,
            "oversize file must not populate the prompt"
        );
    }
}
