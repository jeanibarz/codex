/// The current Codex CLI version as embedded at compile time.
pub const CODEX_CLI_VERSION: &str =
    option_env!("CODEX_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
