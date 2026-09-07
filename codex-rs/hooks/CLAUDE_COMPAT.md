# Claude Code hook compatibility

This document describes the fork's Claude Code hook-discovery compatibility, which
layers on top of the upstream Codex hook system.

## Discovery

When `features.codex_hooks = true`, hook discovery also reads Claude Code settings for the compatible events `PreToolUse`, `PostToolUse`, `UserPromptSubmit`, and `Stop`. Codex discovers `~/.claude/settings.json`, `<cwd>/.claude/settings.json`, and `<cwd>/.claude/settings.local.json` automatically; no manual mirror into `.codex/hooks.json` is required. Claude hook entries that contain an unsupported `if` field are skipped with a startup warning that names the source file, matcher, and dropped count.

## Precedence

Hook sources are loaded from lower to higher precedence in this order:

| Precedence | Source |
| --- | --- |
| 1 | `~/.claude/settings.json` |
| 2 | `<cwd>/.claude/settings.json` |
| 3 | `<cwd>/.claude/settings.local.json` |
| 4 | `$CODEX_HOME/hooks.json` |
| 5 | `<cwd>/.codex/hooks.json` |
| 6 | `[hooks]` in `config.toml` |
| 7 | managed hooks, plugin hooks, and explicit `--settings FILE` hooks |
