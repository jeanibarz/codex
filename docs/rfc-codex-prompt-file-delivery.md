# RFC: Deliver the initial Codex prompt via `--prompt-file`

- **Status:** Draft v3 (revised after parallel critic review + reviewer-specialist round 2)
- **Affects:** Codex fork `feat/claude-compat` (jeanibarz/codex#59) + `~/git/kookr` `src/adapters/codex-cli-adapter.ts` (kookr-ai/kookr#355)
- **Supersedes:** kookr PR #352 (`fix/codex-prompt-race`)

## 1. Summary

Kookr-launched Codex CLI (TUI) sessions intermittently start with **no prompt** —
the agent sits idle until a human intervenes. The fix: kookr writes the initial
prompt to a small launch-artifact file (like the existing `--settings` JSON) and
Codex reads it via a new `--prompt-file <PATH>` flag, which populates the same
`prompt` field a positional CLI argument would. No prompt-size threshold: the
prompt is delivered the same way at 10 bytes or 10 MB. kookr capability-probes
the flag and falls back to a (race-free) positional-argv prompt for binaries
that lack it — see §5.2 and §6 row E.

## 2. Problem & evidence (verified)

### Symptom
A kookr-launched Codex task starts and does nothing; the prompt is never visible.
Reported instance: task `46f60757-8657-4e98-aa99-f21ffbc74b27`.

### Verified facts
- **Task `46f60757`** — `~/.kookr/tasks.json`: `agentType: codex-cli`,
  `status: inProgress`, prompt length **23,468 bytes** (~23 KiB — small; not an
  `ARG_MAX` case).
- **A genuine dropped-prompt rollout exists:**
  `~/.codex/sessions/2026/05/15/rollout-2026-05-15T09-00-32-019e2a6f-….jsonl`
  is a `codex-tui` session containing **exactly one line** (session meta) — it
  started and submitted nothing.
- **The bug is intermittent.** A scan of recent `codex-tui` rollouts shows the
  *majority* received their prompt as the first user message (pre-PR-review
  specialist sessions, checkpoint-resume sessions, etc.). Only some sessions
  drop it. This is the signature of a **race**, not a systematic failure.
- **`E2BIG` for large prompts is real** — kookr issue **#319**: pasting a
  ~100 KiB+ prompt (a Lighthouse report) into the task launcher fails with
  `spawn E2BIG` because the adapter passed the prompt as positional argv.
  PR #337 fixed #319 by moving to terminal-write — which introduced this race.

### Correction to PR #352's evidence
PR #352 cites rollout `…23-27-00…019e2862…` as the bug, reading its first
`user_message: "continue"` as "the user typed continue because the prompt was
dropped." **That reading is wrong.** kookr's ralph-loop relaunches a looped task
with the literal prompt `"continue"` (`ralph-loop-service`:
`launchFreshTaskSession(task, 'continue', …)`). In that rollout the real task
prompt arrives normally as the *second* user message. That rollout is a healthy
ralph continuation, not a dropped prompt. The bug is real (see above) — but
PR #352's specific artifact does not demonstrate it, and its 100 KiB threshold
is calibrated against a misread.

## 3. Root cause

### 3.1 What kookr does today
`CodexCliAdapter.launch()` (after kookr PR #337) builds an argv **without** the
prompt, spawns Codex through the dtach/terminal backend, then calls
`deliverInitialPromptToSession()` — which writes the prompt bytes + `Enter` to
the PTY after spawn.

### 3.2 Why the post-spawn terminal write is racy
Delivering the prompt as PTY bytes *after* spawn depends on Codex's terminal
input pipeline being in the right state at the right moment. Two mechanisms can
each swallow those bytes; this RFC does **not** need to pin which one fires in a
given session, because the proposed fix removes the terminal-write path entirely:

1. **Startup-screen / composer-timing drop.** Before the chat composer exists and
   the thread is configured (banner render, workspace-trust check, MCP server
   startup — kookr `docs/poc/003` §Gap 10; release builds also run
   `run_update_prompt_if_needed`, gated by `#[cfg(not(debug_assertions))]` at
   `tui/src/lib.rs:1216` — confirmed compiled in, since `pnpm codex:rebuild`
   builds `--release`), early PTY bytes are not yet routed to the composer.
2. **Paste-burst misclassification.** The Codex composer uses bracketed-paste /
   burst heuristics; a fast text-then-`Enter` write can be classified as a paste
   (the `Enter` absorbed as a pasted newline) and never submitted. The fork has a
   `disable_paste_burst` guard for exactly this — but see §9: it is currently
   **inert** for kookr sessions.

### 3.3 Why a CLI-argument prompt is structurally race-free
A prompt supplied as the positional `PROMPT` argument never touches the PTY:
`cli.prompt` → `App::run(prompt)` → `initial_prompt` → `create_initial_user_message()`
→ `ChatWidget.initial_user_message`, submitted by the widget once the thread is
ready. It is also non-empty, so it sets `skip_update_prompt`. `--prompt-file`
feeds this exact path by populating `cli.prompt` — it inherits the race-free
property without re-implementing anything.

### 3.4 Why not PR #352
PR #352 keeps **both** paths and branches on a 100 KiB size threshold. It works,
but: it keeps the racy terminal-write path alive for large prompts; it adds a
runtime branch on a magic number (calibrated against the misread evidence of
§2); and it still places the prompt on the process command line for the common
small-prompt case.

## 4. Goals / non-goals

**Goals:** one size-independent delivery mechanism; deterministic (no dependence
on TUI timing or PTY state); handles the `E2BIG` case (#319) by construction;
minimal, contained change.

**Non-goals:** changing follow-up `sendInput` delivery (genuine interactive
input, sent when the TUI *is* ready); changing the Claude Code adapter (its
terminal-write path works); `codex exec` headless mode.

## 5. Proposed solution

### 5.1 Codex fork: add `--prompt-file <PATH>`
A general CLI option — read the initial prompt from a file instead of the
positional `PROMPT`.

`codex-rs/tui/src/cli.rs` — new field on `Cli`, beside `prompt`:

```rust
/// Read the initial user prompt from a file instead of the positional
/// PROMPT argument. The path is tiny in argv (no ARG_MAX limit) and is read
/// at startup, so an orchestrator launching Codex in a PTY can deliver the
/// prompt without a terminal-input race.
#[arg(long = "prompt-file", value_name = "PATH", conflicts_with = "prompt")]
pub prompt_file: Option<std::path::PathBuf>,
```

Resolution — a single helper that folds `--prompt-file` into the `prompt` field
**at every point a `TuiCli` is finalized for use**, so all downstream code (and
both prompt consumers) sees only `prompt`:

```rust
/// Fold --prompt-file into the positional prompt slot. After this runs the
/// file-sourced prompt is indistinguishable from a positional CLI prompt.
fn resolve_prompt_file(cli: &mut TuiCli) -> std::io::Result<()> {
    if cli.prompt.is_none()
        && let Some(path) = cli.prompt_file.take()
    {
        cli.prompt = Some(std::fs::read_to_string(&path)?);
    }
    Ok(())
}
```

Call it in `run_interactive_tui()` **before** the existing CRLF normalization at
`main.rs:1881` (which then covers file content for free — no new normalization
code). The two correctness traps the critic review found are explicitly handled:

- **`merge_interactive_cli_flags` (`main.rs:2007`) destructures `TuiCli` with `..`**
  — a `--prompt-file` passed to `codex resume`/`codex fork` would be silently
  dropped. Fix: add `prompt_file` to that destructure and merge it like `prompt`.
  (kookr's adapter launches the *base* `codex` command and never resumes, so this
  is not on kookr's hot path — but leaving a silently-dropped flag is the exact
  failure class this RFC exists to kill.)
- **`run_debug_prompt_input_command` (`main.rs:1543`)** also reads
  `interactive.prompt`. It calls `resolve_prompt_file` too (or rejects the flag).

On a missing/unreadable file, `resolve_prompt_file` returns an error that becomes
`AppExitInfo::fatal`; `handle_app_exit` (verified, `main.rs:657`) prints
`ERROR: …` to stderr and `exit(1)`. Codex fails fast and visibly instead of
starting an empty session.

That is the entire Codex change: one field, one ~6-line helper, two call sites,
one extra line in `merge_interactive_cli_flags`. No change to `App::run`,
`create_initial_user_message`, submission, or `skip_update_prompt`.

### 5.2 Kookr: write the prompt as a launch artifact
`src/adapters/codex-cli-adapter.ts` — treat the prompt like the `--settings`
JSON: write it next to the settings file, pass its path, drop the terminal
write. Whether to use `--prompt-file` is **capability-probed** once per adapter
(same `probeBinaryFlagSupport` mechanism the adapter already uses for
`--plugin-dir`); a binary that does not advertise the flag falls back to a
positional argv prompt (see §6 row E — adopted after review round 2).

```ts
const promptFileSupported = await this.probePromptFileSupport();
const promptPath = `${this.settingsDir}/${tmuxName}.prompt.txt`;
if (promptFileSupported && this.writeFile) {
  await this.writeFile(promptPath, promptWithCheckpoint);
}

const args = ['-c', 'features.codex_hooks=true', permissionFlagStr,
              '--settings', settingsPath];
if (promptFileSupported) args.push('--prompt-file', promptPath);
// … existing --plugin-dir injection unchanged …
if (!promptFileSupported) args.push(promptWithCheckpoint); // positional fallback, last

await this.backend.createSession({ id: tmuxName, command: this.agentBin, args, env, cwd, size });
// deliverInitialPromptToSession() call REMOVED for codex.
```

- Remove the `deliverInitialPromptToSession` import from the codex adapter (the
  function stays — the Claude Code adapter still uses it).
- The prompt file shares the **exact lifecycle of the settings file**: written
  via `this.writeFile`, referenced by absolute path, **not cleaned up** on
  `stop()`. Codex reads it once at startup; no cleanup code is added (matching
  the settings file avoids an unlink-vs-read race and keeps the change minimal —
  see §7 M3). Stale-artifact reaping for `settingsDir` is a pre-existing,
  separate hygiene item.
- kookr PR #352 is **closed** in favor of this; its `CODEX_PROMPT_ARGV_THRESHOLD_BYTES`
  constant and boundary tests are never introduced.

### 5.3 Why this is race-free
- kookr `await`s the file write **before** `createSession()`, so the file is
  fully written before Codex is spawned. The file lives on the local Linux
  filesystem (`~/.kookr/…`, ext4 — *not* a `/mnt/c` 9p mount), so its page cache
  is coherent across processes; a read after the completed write sees the data.
  This is the **same property the existing `--settings <path>` already relies
  on** — if it did not hold, `--settings` would already be broken.
- The argv carries only a short path ⇒ no `ARG_MAX`/`E2BIG` at any prompt size.
- The file-sourced prompt populates `cli.prompt` ⇒ identical to a positional
  prompt: no PTY, no composer timing, no paste heuristic, and `skip_update_prompt`
  is set.
- The `--plugin-dir`-style capability probe means the *intended* config (the
  kookr-fork) always takes the single `--prompt-file` path; the positional-argv
  fallback is reached only by binaries lacking the flag, and is itself race-free
  (argv, not a terminal write) — just bounded by `ARG_MAX`. This is a
  **capability** branch, not the prompt-*size* branch PR #352 was rejected for.

## 6. Alternatives considered

| Alternative | Verdict | Why |
|---|---|---|
| **A. kookr PR #352** — branch on a 100 KiB size threshold | Rejected | Keeps the racy terminal-write path for large prompts; runtime branch on a magic number; threshold calibrated against misread evidence (§2); prompt still in argv for small prompts. |
| **B. Always deliver via positional argv** (revert PR #337) | Rejected | `E2BIG` is **verified real** (issue #319 — a ~100 KiB+ pasted prompt). Always-argv reintroduces #319. |
| **C. Fix the TUI to not drop early PTY input** | Rejected | Honestly: correctly buffering raw terminal input across the alt-screen switch, bracketed-paste enable, trust check, and composer construction is timing-dependent and complex, and would still not address `E2BIG`. `--prompt-file` is deterministic and size-independent. Note: this RFC *routes around* the TUI's early-input fragility rather than fixing it; that fragility still affects other terminal-write callers (see §9, and `sendInput` follow-ups). Fixing the TUI input pipeline remains worthwhile as separate hardening — it is just not the right vehicle for reliable *initial-prompt* delivery. |
| **D. Prompt on stdin** | Rejected | The interactive TUI owns stdin/the PTY for the whole session; there is no separate stdin channel for a one-shot prompt. |
| **E. Capability-probe `--prompt-file` like `--plugin-dir`** | **Adopted** (review round 2) | Initially rejected — "prompt delivery is core, the codexcli agent already requires the fork." A reviewer specialist flagged that unconditional `--prompt-file` hard-fails any binary lacking the flag (stock codex, or a fork built before this change), regressing graceful degradation. The original rejection missed that **positional argv is a clean race-free fallback** (it only fails on >~`ARG_MAX` prompts). So: probe `--prompt-file` once per adapter (existing `probeBinaryFlagSupport` mechanism); supported ⇒ `--prompt-file`; unsupported ⇒ positional argv + one-time warning. Capability branch, not a prompt-size branch. |

## 7. Edge cases & failure modes (from critic review)

- **`--prompt-file` + positional `PROMPT` both given:** `conflicts_with = "prompt"`
  rejects at parse time for one `TuiCli`. (Caveat: across the base CLI and a
  flattened subcommand `TuiCli` they are separate matchers; resolving
  `prompt_file` early — §5.1 — means the merge only ever sees `prompt`.)
- **Missing/unreadable file:** fatal error + `exit(1)` (§5.1). In the happy path
  kookr `await`s the write before spawn, so the file exists. kookr has no
  post-`createSession` liveness check today; a Codex early-exit is still
  observable as the dtach session ending. Adding an explicit liveness check is a
  reasonable separate hardening, out of scope here.
- **Empty file:** yields `cli.prompt = Some("")`, identical to `codex ""`. kookr
  never writes an empty prompt.
- **Non-UTF-8 content:** `read_to_string` fails ⇒ fatal. kookr prompts are
  UTF-8 task text; a non-UTF-8 prompt file is a kookr bug, and failing fast is
  acceptable (the old lossy `TextDecoder` path silently corrupted instead).
- **Large file:** `std::fs::read_to_string` is a blocking read on the startup
  path; for a multi-MB prompt this is tens of milliseconds before TUI init —
  acceptable. (`tokio::fs` is available if ever measured to matter.)
- **`tmuxName` collision (M3):** `tmuxName` is an 8-hex-char UUID slice; the
  prompt file is keyed on it like the settings file, so it inherits the same
  (pre-existing) collision surface — not made materially worse. Lengthening the
  session-name key is a separate hygiene fix.
- **Data at rest (M4/Q9):** the prompt file holds the full task text under
  `~/.kookr/settings/` at the `writeFile` default mode. This is a real change vs
  transient argv. On a single-user dev host the exposure is low and equal in
  kind to the settings file. Tightening `writeFile` to `0600` (it takes no mode
  today) is a worthwhile **separate** hygiene change; this RFC does not block on
  it and does not claim it.
- **`writeFile` not injected (tests):** the settings file already depends on
  `this.writeFile`; the prompt file inherits the identical contract — no new
  asymmetry. Production always injects `writeFile`.

## 8. Test plan

**Codex fork**
- `--prompt-file` populates `prompt`; `conflicts_with` rejects file+positional;
  missing file ⇒ `ExitReason::Fatal` ⇒ `exit(1)`; CRLF in file ⇒ normalized.
- `merge_interactive_cli_flags` carries `prompt_file` (regression test:
  `codex resume --prompt-file X` is not silently dropped).
- Integration: `codex --prompt-file p.txt` starts a session whose first
  `user_message` equals the file content and skips the update-prompt screen.

**Kookr**
- `codex-cli-adapter.test.ts`: `launch()` writes `${tmuxName}.prompt.txt` with
  the checkpoint-prefixed prompt; argv contains `--prompt-file <path>` and **no**
  trailing positional prompt; `deliverInitialPromptToSession` is **not** called.
- Manual: relaunch a codex task; the new rollout's first `user_message` equals
  the task prompt.

## 9. Companion fix (independent, recommended): `KOOKR_TASK_ID`

The fork's paste-burst guard reads env var **`LOOPER_TASK_ID`**
(`tui/src/chatwidget/constructor.rs:278`), but kookr exports **`KOOKR_TASK_ID`**.
So `effective_disable_paste_burst` never fires for kookr sessions — the
`disable_paste_burst` guard built for programmatic input is dead code in
production. Fix: make `looper_task_id_env()` also accept `KOOKR_TASK_ID`.

This is **not** a substitute for `--prompt-file`: it cannot fix the `E2BIG`
case (#319), and it does not help if the drop happens before the composer
exists (§3.2 mechanism 1; e.g. the 1-line rollout in §2). It is, however, a
real latent bug that still affects fast `sendInput` follow-ups, so it is worth
landing alongside this RFC as a one-line change — tracked separately to keep
this change minimal.

## 10. Rollout

1. Land `--prompt-file` in the Codex fork (`feat/claude-compat`) — jeanibarz/codex#59.
2. `pnpm codex:rebuild` so `KOOKR_CODEX_BIN` advertises the flag.
3. Land the kookr `codex-cli-adapter.ts` change (kookr-ai/kookr#355); close PR #352.
4. `pnpm prod:update`.

Because the kookr adapter **capability-probes** `--prompt-file` (§5.2, §6 row E),
deploy order is **not strict**: a kookr build running against a not-yet-rebuilt
codex binary detects the missing flag and falls back to a positional-argv prompt
(race-free, `ARG_MAX`-bounded) with a one-time warning — no hard failure, no
intermittent drop. Rebuilding the codex binary (steps 1–2) is what unlocks the
file-artifact path and therefore large-prompt / `E2BIG` coverage; until then the
fallback keeps normal-size prompts working.

**Upstreaming:** `--prompt-file` is a general, non-kookr-specific feature and a
good candidate to submit to `openai/codex`. Until then it is fork weight, but a
small one (one `cli.rs` field + one helper) and low rebase-conflict surface.

## Appendix: verification performed

- `~/.kookr/tasks.json` — task `46f60757`: `codex-cli`, `inProgress`, 23 KiB prompt.
- `~/.codex/sessions/.../rollout-…09-00-32-019e2a6f….jsonl` — 1-line `codex-tui`
  rollout (dropped-prompt instance).
- Rollout scan — most recent `codex-tui` sessions received their prompt fine ⇒
  intermittent ⇒ race.
- kookr issue #319 / PR #337 — `E2BIG` for large prompts is real and was the
  reason PR #337 moved off argv.
- `ralph-loop-service` — `"continue"` is a ralph relaunch prompt (corrects
  PR #352's evidence reading).
- `scripts/rebuild-codex.sh` — `kookr-dev` profile builds `--release`
  (`debug_assertions` off ⇒ `skip_update_prompt` block compiled in).
- `codex-rs/cli/src/main.rs` — `run_interactive_tui` (1875), `handle_app_exit`
  (657, `Fatal ⇒ exit(1)`), `merge_interactive_cli_flags` (2007, `..` drop),
  `run_debug_prompt_input_command` (1543, second `prompt` consumer).
- `codex-rs/tui/src/{cli.rs,lib.rs,app.rs}` — `prompt` flow into
  `create_initial_user_message`; confirmed no existing `--prompt-file`.
