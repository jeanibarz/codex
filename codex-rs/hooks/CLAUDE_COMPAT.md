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

## Provider progress

While sampling, nonempty provider text, reasoning and tool-input deltas can emit
`Notification` with `notification_type: "provider_progress"`. This includes
custom tool input with no argument-diff consumer. The payload contains the
ordinary session and turn identity, a fixed message and `observed_at_ms` (Unix
milliseconds when output arrived); it never contains model output.

A sampling-scoped worker keeps one in-flight delivery and one coalesced pending
observation. It sends at most once every ten seconds, discards observations
older than ten seconds, and bounds each delivery to two seconds. Configured
asynchronous command handlers remain under that same deadline and cancellation scope.
Progress command descendants are also terminated when the root shell exits,
so a background child cannot outlive delivery.
MCP handlers are skipped for this notification because dropping their local
future cannot guarantee cancellation of the remote work.
Completion or cancellation drops pending work. Hook lifecycle events are not
persisted for progress, so slow handler activity cannot look like model output.

Consumers must use the original observation time, validate session/turn
ownership, and ignore historical replay. A timer or transport keepalive is not
provider progress. Other notification types retain their existing dispatch.
