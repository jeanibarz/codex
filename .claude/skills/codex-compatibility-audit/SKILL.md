---
name: codex-compatibility-audit
description: Iteratively analyze Claude Code vs Codex CLI feature gaps and maintain candidate contribution issues in jeanibarz/codex fork. Designed for Ralph Wiggum loop execution.
keywords: codex, claude-code, compatibility, contribution, fork, issues, audit, feature-gap, openai, anthropic
related: github-issue-workflow, session-reflect
---

# Codex CLI Compatibility Audit

Maintain a curated set of candidate contribution ideas for making OpenAI's Codex CLI more compatible with Claude Code. Issues live in the `jeanibarz/codex` GitHub fork.

## Purpose

This is an **iterative maintenance task** designed for Ralph Wiggum loop execution. Each run should:
1. Survey the current state (existing issues, recent changes in both tools)
2. Create new candidate ideas discovered since last run
3. Update existing issues with new information
4. Close issues that are no longer relevant (upstream implemented it, idea was wrong)
5. Leave the issue set in good shape for the next run

## Prerequisites

- Fork: `jeanibarz/codex` (forked from `openai/codex`)
- Issues enabled on the fork
- Labels: `compatibility`, `candidate`, `high-impact`, `medium-impact`, `low-impact`

## Step 1: Survey Current State

### 1a. List existing candidate issues

```bash
gh issue list -R jeanibarz/codex --label "candidate" --state open --limit 100 --json number,title,labels,updatedAt
```

### 1b. Check for recent Claude Code changes

Look at recent issues, PRs, and releases in `anthropics/claude-code`:

```bash
# Recent closed issues (new features shipped)
gh issue list -R anthropics/claude-code --state closed --limit 50 --json number,title,closedAt,labels

# Recent open issues (feature requests, bugs)
gh issue list -R anthropics/claude-code --state open --limit 100 --json number,title,labels

# Check README/docs for new features
gh api repos/anthropics/claude-code/contents/README.md -q '.content' | base64 -d | head -200
```

### 1c. Check for recent Codex CLI changes

```bash
# Recent closed issues (features they've added)
gh issue list -R openai/codex --state closed --limit 50 --json number,title,closedAt,labels

# Recent open issues (community requests)
gh issue list -R openai/codex --state open --limit 100 --json number,title,labels

# Check source for new features
gh api repos/openai/codex/contents/codex-rs/core -q '.[].name'
```

## Step 2: Analyze Gaps

For each Claude Code feature, check if Codex CLI:
- **Already has it** -> close the candidate issue if one exists
- **Has a partial implementation** -> update the issue with current state
- **Doesn't have it** -> keep or create a candidate issue

### Key comparison areas

| Area | Claude Code | Codex CLI |
|------|-------------|-----------|
| Instruction files | `CLAUDE.md` (hierarchical) | `AGENTS.md` + `project_doc_fallback_filenames` |
| Hook events | 12+ events (SessionStart/End, Pre/PostToolUse, Stop, StopFailure, PermissionRequest, InstructionsLoaded, TaskCreated/Completed, CwdChanged, FileChanged, PostCompact, Elicitation) | 5 events (SessionStart, PreToolUse, PostToolUse, Stop, UserPromptSubmit) |
| Hook payloads | JSON with session_id, tool_name, tool_input | JSON with session_id, cwd, client, hook_event discriminated union |
| MCP config | `.mcp.json` project file | `config.toml` [mcp_servers] section |
| Conditional rules | `.claude/rules/*.md` with `paths:` glob frontmatter | No equivalent |
| Worktree isolation | EnterWorktree/ExitWorktree tools, `--worktree` flag | Ghost commits, no explicit worktree tools |
| Slash commands | `.claude/commands/*.md` with frontmatter | Skills-based, different format |
| Plan mode | `/plan`, EnterPlanMode/ExitPlanMode, structured review | May vary |
| Plugin format | `.claude-plugin/plugin.json` directory structure | Own plugin format |
| Context mgmt | `/context` command, auto-compaction with circuit breaker | Auto-compaction |
| Memory format | Markdown files (human-readable, git-friendly) | SQLite 2-phase pipeline |
| Deferred tools | ToolSearch pattern | May vary |
| @-file refs | `@` autocomplete in prompt | May vary |
| Cron/loop | `/loop`, CronCreate/Delete/List | May vary |
| Status line | Custom scripts receiving JSON | May vary |

## Step 3: Maintain Issues

### Creating a new issue

Use this template:

```bash
gh issue create -R jeanibarz/codex \
  --title "Brief description of the compatibility feature" \
  --label "compatibility,candidate,{impact}" \
  --assignee jeanibarz \
  --body "$(cat <<'EOF'
## Context

What Claude Code has and what Codex CLI lacks (or does differently).

## Proposal

Concrete change to make in Codex CLI.

## Why this matters

- User-facing benefit
- Ecosystem benefit
- Implementation feasibility

## Implementation notes

- Key files/crates to modify
- Dependencies on other changes
- Risks or trade-offs
EOF
)"
```

### Updating an existing issue

```bash
gh api repos/jeanibarz/codex/issues/{number} -X PATCH -f body="updated body"
```

Or add a comment with new information:

```bash
gh api repos/jeanibarz/codex/issues/{number}/comments -X POST -f body="Update: ..."
```

### Closing an issue

When Codex CLI implements a feature upstream, or the idea is no longer relevant:

```bash
gh issue close -R jeanibarz/codex {number} -c "Closed: {reason}"
```

### Relabeling impact

```bash
gh api repos/jeanibarz/codex/issues/{number}/labels -X PUT --input - <<'EOF'
{"labels":["compatibility","candidate","high-impact"]}
EOF
```

## Step 4: Verify Consistency

After making changes, verify the issue set:

```bash
# Count by impact
gh issue list -R jeanibarz/codex --label "high-impact" --state open --json number | jq length
gh issue list -R jeanibarz/codex --label "medium-impact" --state open --json number | jq length
gh issue list -R jeanibarz/codex --label "low-impact" --state open --json number | jq length

# List all open candidates
gh issue list -R jeanibarz/codex --label "candidate" --state open --json number,title,labels
```

## Idempotency Rules (Ralph Wiggum Loop)

This task is designed to be run repeatedly with the same prompt. Each run should be safe:

1. **Don't create duplicate issues.** Always check existing issues before creating new ones.
2. **Don't update issues that haven't changed.** Only comment/update when there's new information.
3. **Converge, don't diverge.** Each run should refine the issue set, not grow it unboundedly.
4. **Check upstream first.** Before creating an issue, verify Codex hasn't already implemented it.
5. **Date-stamp comments.** When adding update comments, include the date for tracking.
6. **Respect existing triage.** If the user has manually relabeled or modified an issue, don't override their changes.

## Quality Criteria for Candidate Issues

A good candidate issue should be:

- **Feasible**: Can be implemented in a single PR (or a small series)
- **Valuable**: Benefits Codex CLI users, not just Claude Code compatibility for its own sake
- **Specific**: Clear proposal with implementation pointers, not vague "add feature X"
- **Upstream-friendly**: Likely to be accepted by openai/codex maintainers
- **Non-duplicative**: Not already requested in openai/codex issues

## Anti-Patterns

- Don't propose changes that break Codex's existing behavior or philosophy
- Don't suggest copying proprietary Claude Code features — focus on interoperability
- Don't create issues for trivial differences (config format, naming conventions)
- Don't propose changes that only benefit Claude Code users at the expense of Codex users
- Don't create massive "umbrella" issues — keep each issue focused and implementable
