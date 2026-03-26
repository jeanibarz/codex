---
name: codex-bug-triage
description: Browse upstream openai/codex bug reports, score them by reproducibility and confidence, and maintain ranked triage issues in jeanibarz/codex fork. Designed for Ralph Wiggum loop execution.
keywords: codex, bug, triage, reproduce, upstream, openai, priority, ranking, issue, fix, contribution
related: codex-compatibility-audit, github-issue-workflow
---

# Codex CLI Bug Triage

Browse user-reported bugs in `openai/codex`, score each by confidence and reproducibility, and maintain a ranked set of triage issues in `jeanibarz/codex` for future reproduction and fix attempts.

## Purpose

This is an **iterative maintenance task** designed for Ralph Wiggum loop execution. Each run should:
1. Fetch new upstream bug reports since last run
2. Score each bug for confidence (is it real?) and reproducibility (can we reproduce it?)
3. Create/update triage issues in the fork with priority scores in the title
4. Re-score existing triage issues if new information appeared upstream
5. Close triage issues for bugs that were fixed upstream or proven invalid

## Prerequisites

- Fork: `jeanibarz/codex` (forked from `openai/codex`)
- Issues enabled on the fork
- Labels: `bug-triage`, `confidence-high`, `confidence-medium`, `confidence-low`, `repro-easy`, `repro-medium`, `repro-hard`

## Step 1: Fetch Upstream Bug Reports

### 1a. Get open bugs from upstream

```bash
# Fetch bugs with the "bug" label (or similar)
gh issue list -R openai/codex --label "bug" --state open --limit 200 --json number,title,body,labels,comments,createdAt,updatedAt

# Also search for issues that look like bug reports but may not be labeled
gh issue list -R openai/codex --state open --limit 200 --json number,title,labels,createdAt --jq '[.[] | select(.title | test("bug|crash|error|fail|broken|regression|fix"; "i"))]'
```

### 1b. Get recently closed bugs (to close our triage issues)

```bash
gh issue list -R openai/codex --label "bug" --state closed --limit 100 --json number,title,closedAt
```

### 1c. Get existing triage issues in our fork

```bash
gh issue list -R jeanibarz/codex --label "bug-triage" --state open --limit 200 --json number,title,body,labels,updatedAt
```

## Step 2: Score Each Bug

### Confidence Score (1-5): Is this a real bug?

Score how likely the reported issue is a genuine bug vs. user error, misconfiguration, or intended behavior.

| Score | Meaning | Signals |
|-------|---------|---------|
| **5** | Definitely a bug | Maintainer confirmed, multiple reporters, clear stack trace, regression from known-good version |
| **4** | Very likely a bug | Detailed repro steps, consistent with codebase understanding, affects common workflow |
| **3** | Probably a bug | Single reporter but credible description, plausible root cause, no contradicting evidence |
| **2** | Unclear | Vague description, could be config issue, might be expected behavior, single report without repro |
| **1** | Probably not a bug | Looks like user error, documented limitation, or feature request disguised as bug |

**Signals that increase confidence:**
- Multiple users report the same issue
- Maintainer or contributor commented acknowledging the bug
- Clear error message or stack trace provided
- Issue includes a minimal reproduction case
- Regression identified (worked in version X, broken in Y)
- Related to recently changed code (check with `gh pr list -R openai/codex --state merged --limit 20`)

**Signals that decrease confidence:**
- Reporter is confused about expected behavior
- Issue is about a feature behind an experimental flag
- Environment-specific (unusual OS, Docker, WSL edge case)
- No reproduction steps provided
- Contradicted by other users ("works for me")

### Reproducibility Score (1-5): How easy to reproduce?

Score how easy it would be for us to reproduce the bug locally.

| Score | Meaning | Signals |
|-------|---------|---------|
| **5** | Trivial to reproduce | Clear steps, common setup (Linux/macOS, standard shell), deterministic |
| **4** | Easy to reproduce | Clear steps but needs specific setup (particular project structure, MCP server, etc.) |
| **3** | Moderate effort | Steps exist but involve multi-step setup, timing-dependent, or platform-specific |
| **2** | Hard to reproduce | Intermittent, requires rare conditions, complex environment, or unclear steps |
| **1** | Very hard / impossible | No repro steps, race condition, specific hardware, or requires access we don't have |

**Factors that make reproduction easier:**
- Detailed step-by-step reproduction instructions
- Minimal reproduction case provided (small project, specific command)
- Affects default configuration (no special settings needed)
- Works on common platforms (Linux x86_64, macOS ARM64)
- Deterministic (happens every time)

**Factors that make reproduction harder:**
- Requires specific platform (Windows, WSL2, specific Linux distro)
- Involves timing / race conditions
- Needs specific third-party tools or MCP servers
- Requires paid API access or specific account type
- Intermittent ("happens sometimes")

### Priority Score Calculation

```
priority = confidence * reproducibility
```

| Priority Range | Meaning |
|----------------|---------|
| **20-25** | Top priority — real bug, easy to reproduce. Fix first. |
| **12-19** | High priority — worth investigating soon |
| **6-11** | Medium priority — investigate when time permits |
| **1-5** | Low priority — park unless more evidence appears |

## Step 3: Create/Update Triage Issues

### Issue title format

```
[P{priority}] {upstream_issue_title} (openai/codex#{upstream_number})
```

Examples:
- `[P25] Sandbox crashes on symlinked /tmp directory (openai/codex#15234)`
- `[P12] MCP OAuth flow fails with custom redirect URI (openai/codex#15456)`
- `[P4] TUI flickering on Alacritty with 144Hz monitor (openai/codex#15789)`

### Issue body template

```markdown
## Upstream Issue

- **Source**: openai/codex#{number}
- **Reporter**: @{author}
- **Created**: {date}
- **Status**: {open/closed}
- **Upstream labels**: {labels}

## Scores

| Dimension | Score | Reasoning |
|-----------|-------|-----------|
| **Confidence** | {1-5}/5 | {brief justification} |
| **Reproducibility** | {1-5}/5 | {brief justification} |
| **Priority** | **{confidence * reproducibility}**/25 | |

## Summary

{One-paragraph summary of the bug in our own words}

## Reproduction Plan

{What we would need to do to reproduce this locally — specific steps, environment setup}

## Potential Root Cause

{Our analysis of what might be causing this, based on the upstream discussion and source code references}

## Fix Approach (if obvious)

{If the fix is apparent from the discussion or code, outline it. Otherwise "Needs investigation after reproduction."}

## Upstream Activity

- {date}: {key comment or event from upstream discussion}
- ...
```

### Creating a new triage issue

```bash
gh issue create -R jeanibarz/codex \
  --title "[P{score}] {title} (openai/codex#{number})" \
  --label "bug-triage,confidence-{level},repro-{level}" \
  --assignee jeanibarz \
  --body "$(cat <<'EOF'
{body from template above}
EOF
)"
```

### Updating an existing triage issue

When re-scoring (new upstream info, upstream status change):

```bash
# Update title with new priority score
gh api repos/jeanibarz/codex/issues/{number} -X PATCH -f title="[P{new_score}] {title} (openai/codex#{upstream})"

# Update labels
gh api repos/jeanibarz/codex/issues/{number}/labels -X PUT --input - <<'EOF'
{"labels":["bug-triage","confidence-{new_level}","repro-{new_level}"]}
EOF

# Add update comment
gh api repos/jeanibarz/codex/issues/{number}/comments -X POST -f body="**Re-scored ({date})**: Priority {old} → {new}. Reason: {what changed}"
```

### Closing a triage issue

```bash
gh issue close -R jeanibarz/codex {number} -c "Closed: {reason — fixed upstream in openai/codex#{pr}, or confirmed not a bug}"
```

## Step 4: Verify Consistency

After making changes:

```bash
# Count by priority tier
gh issue list -R jeanibarz/codex --label "bug-triage" --state open --json title --jq '[.[] | select(.title | test("\\[P(2[0-5])\\]"))] | length' # top priority
gh issue list -R jeanibarz/codex --label "bug-triage" --state open --json title --jq '[.[] | select(.title | test("\\[P(1[2-9])\\]"))] | length' # high priority

# Full list sorted by title (priority score is first, so lexicographic sort ≈ priority sort)
gh issue list -R jeanibarz/codex --label "bug-triage" --state open --json number,title --jq 'sort_by(.title) | reverse | .[] | "\(.number)\t\(.title)"'

# Verify no orphans (triage issue exists but upstream was closed)
# For each triage issue, extract the upstream number and check its status
```

## Idempotency Rules (Ralph Wiggum Loop)

1. **Don't create duplicate triage issues.** Before creating, check if a triage issue already exists for the upstream issue number (search body for `openai/codex#{number}`).
2. **Don't re-score without new information.** Only update scores when upstream has new comments, status changes, or labels since the triage issue was last updated.
3. **Converge, don't diverge.** Each run should refine scores and close resolved issues, not grow the list unboundedly.
4. **Respect manual overrides.** If the user manually changed a score or label, don't override it. Detect this by checking if the issue body was edited more recently than the last automated comment.
5. **Date-stamp all updates.** Every comment should include the date for tracking.
6. **Batch wisely.** Don't process all 15,000+ upstream issues at once. Focus on: recently opened (last 30 days), recently updated, and high-engagement (many comments/reactions).

## Filtering Strategy

The upstream repo has thousands of issues. Use these filters to find the most actionable bugs:

### High-value upstream queries

```bash
# Bugs with many reactions (community agrees it's a problem)
gh api "repos/openai/codex/issues?labels=bug&state=open&sort=reactions-+1&direction=desc&per_page=50"

# Recently opened bugs (fresh, likely reproducible on current version)
gh issue list -R openai/codex --label "bug" --state open --limit 50 --json number,title,body,createdAt,comments --jq 'sort_by(.createdAt) | reverse'

# Bugs with detailed reproduction steps (search for "steps to reproduce" or "repro")
gh api "repos/openai/codex/issues?labels=bug&state=open&per_page=100" --jq '.[] | select(.body | test("step|repro|reproduce|minimal"; "i")) | {number, title}'

# Bugs affecting core functionality (sandbox, exec, MCP)
gh api "repos/openai/codex/issues?labels=bug&state=open&per_page=100" --jq '.[] | select(.title | test("sandbox|exec|mcp|crash|panic"; "i")) | {number, title}'
```

### Skip criteria

Don't create triage issues for:
- Feature requests disguised as bugs
- Issues specific to ChatGPT account/billing
- Issues about the legacy Node.js CLI (only Rust CLI matters)
- Issues requiring Windows-only reproduction (unless we have Windows available)
- Issues already assigned to a maintainer with an active PR fixing them

## Anti-Patterns

- Don't score based on title alone — always read the issue body and comments
- Don't create triage issues for every upstream bug — focus on ones we could realistically fix
- Don't copy the entire upstream issue body into our triage issue — summarize in our own words
- Don't ignore upstream discussion — it often reveals whether the bug is real and hints at root cause
- Don't set all scores to 3 by default — differentiate aggressively to make the ranking useful
