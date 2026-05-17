use codex_tui::Cli as TuiCli;
use std::io::Read;

// Keep prompt-file reads bounded so accidentally passing a large log or binary
// cannot allocate unbounded memory before the TUI starts.
const MAX_PROMPT_FILE_BYTES: u64 = 10 * 1024 * 1024;

pub(crate) fn normalize_prompt(prompt: String) -> String {
    prompt.replace("\r\n", "\n").replace('\r', "\n")
}

pub(crate) fn resolve_prompt_file(interactive: &mut TuiCli) -> std::io::Result<()> {
    if interactive.prompt.is_some() {
        return Ok(());
    }

    let Some(path) = interactive.prompt_file.take() else {
        return Ok(());
    };

    let read_err = |err: std::io::Error| {
        std::io::Error::new(
            err.kind(),
            format!("failed to read --prompt-file {}: {err}", path.display()),
        )
    };
    let metadata = std::fs::metadata(&path).map_err(&read_err)?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("--prompt-file {} is not a regular file", path.display()),
        ));
    }

    let file = std::fs::File::open(&path).map_err(&read_err)?;
    let metadata = file.metadata().map_err(&read_err)?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("--prompt-file {} is not a regular file", path.display()),
        ));
    }

    let mut bytes = Vec::new();
    file.take(MAX_PROMPT_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(&read_err)?;
    if bytes.len() as u64 > MAX_PROMPT_FILE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "--prompt-file {} is over the {MAX_PROMPT_FILE_BYTES}-byte limit",
                path.display()
            ),
        ));
    }

    let text = String::from_utf8(bytes).map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("failed to read --prompt-file {}: {err}", path.display()),
        )
    })?;
    interactive.prompt = Some(normalize_prompt(text));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use pretty_assertions::assert_eq;
    use std::io::Write;
    use std::path::Path;

    fn cli_with_prompt_file(path: &Path) -> TuiCli {
        TuiCli::try_parse_from(["codex", "--prompt-file", path.to_str().expect("utf-8 path")])
            .expect("parse should succeed")
    }

    #[test]
    fn reads_utf8_file_into_prompt() {
        let mut file = tempfile::NamedTempFile::new().expect("create temp prompt file");
        write!(file, "first line\nsecond line").expect("write temp prompt");
        let mut cli = cli_with_prompt_file(file.path());

        resolve_prompt_file(&mut cli).expect("resolve should succeed");

        assert_eq!(cli.prompt.as_deref(), Some("first line\nsecond line"));
        assert_eq!(cli.prompt_file, None);
    }

    #[test]
    fn rejects_missing_file() {
        let mut cli = TuiCli::try_parse_from([
            "codex",
            "--prompt-file",
            "/nonexistent/codex-prompt-file/missing.txt",
        ])
        .expect("parse should succeed");

        let err = resolve_prompt_file(&mut cli).expect_err("missing file must error");

        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert!(
            err.to_string().contains("--prompt-file"),
            "error should name the flag: {err}"
        );
    }

    #[test]
    fn rejects_oversized_file() {
        let file = tempfile::NamedTempFile::new().expect("create temp prompt file");
        file.as_file()
            .set_len(MAX_PROMPT_FILE_BYTES + 1)
            .expect("size temp prompt file");
        let mut cli = cli_with_prompt_file(file.path());

        let err = resolve_prompt_file(&mut cli).expect_err("oversized file must error");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(cli.prompt, None);
        assert!(
            err.to_string().contains("limit"),
            "error should mention the size limit: {err}"
        );
    }

    #[test]
    fn rejects_non_regular_file() {
        let dir = tempfile::tempdir().expect("create temp prompt dir");
        let mut cli = cli_with_prompt_file(dir.path());

        let err = resolve_prompt_file(&mut cli).expect_err("directory must error");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(
            err.to_string().contains("regular file"),
            "error should mention regular file requirement: {err}"
        );
    }

    #[test]
    fn rejects_invalid_utf8() {
        let mut file = tempfile::NamedTempFile::new().expect("create temp prompt file");
        file.write_all(&[0xff, 0xfe, 0xfd])
            .expect("write invalid utf-8");
        let mut cli = cli_with_prompt_file(file.path());

        let err = resolve_prompt_file(&mut cli).expect_err("invalid utf-8 must error");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(
            err.to_string().contains("--prompt-file"),
            "error should name the flag: {err}"
        );
    }

    #[test]
    fn normalizes_line_endings_like_positional_prompt() {
        let mut file = tempfile::NamedTempFile::new().expect("create temp prompt file");
        write!(file, "a\r\nb\rc").expect("write temp prompt");
        let mut cli = cli_with_prompt_file(file.path());

        resolve_prompt_file(&mut cli).expect("resolve should succeed");

        assert_eq!(cli.prompt.as_deref(), Some("a\nb\nc"));
        assert_eq!(
            normalize_prompt("a\r\nb\rc".to_string()),
            "a\nb\nc".to_string()
        );
    }
}
