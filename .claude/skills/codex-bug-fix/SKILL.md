---
name: codex-bug-fix
description: Pick the highest-priority triage issue from jeanibarz/codex, reproduce the bug, implement a minimal fix, and prepare it for PR. Uses adversarial subagent critique at each stage.
keywords: codex, bug, fix, reproduce, worktree, upstream, openai, contribution, patch, critique
related: codex-bug-triage, github-issue-workflow, pre-pr-review, git-commit-discipline
---

# Codex CLI Bug Fix

Pick the highest-priority open triage issue from `jeanibarz/codex`, reproduce the upstream bug, implement a minimal fix, and prepare it for upstream PR review.

## Purpose

This is a **single-execution task** (not a loop). Each run:
1. Picks the top-priority `bug-triage` issue
2. Validates it is still open upstream and has no existing fix PR
3. Reproduces the bug in a dedicated worktree
4. Implements the minimal correct fix
5. Documents everything on the issue for reviewer confidence

## Prerequisites

- Fork: `jeanibarz/codex` (forked from `openai/codex`)
- Triage issues exist (created by `codex-bug-triage` skill)
- Labels: `bug-triage`, `ready-for-pr`
- Codex CLI source builds locally (`cargo build` in `codex-rs/`)

---

## Phase 1: Select and Validate

### 1a. Pick the highest-priority open triage issue

```bash
gh api repos/jeanibarz/codex/issues \
  --jq '[.[] | select(.labels | map(.name) | index("bug-triage")) | select(.labels | map(.name) | index("ready-for-pr") | not)] | sort_by(.title) | reverse | .[0]'
```

The `[P{score}]` prefix in the title means lexicographic reverse sort gives highest priority first.

Extract:
- The fork issue number
- The upstream issue number (from title: `openai/codex#{N}`)
- The priority score
- The issue body (scores, summary, reproduction plan, root cause)

### 1b. Assign the issue

```bash
gh api repos/jeanibarz/codex/issues/{fork_number} -X PATCH -f assignee=jeanibarz
```

### 1c. Check for existing upstream fix

Search for PRs that reference the upstream issue number:

```bash
# Search merged PRs
gh api "repos/openai/codex/pulls?state=closed&per_page=100" \
  --jq '[.[] | select(.merged_at != null) | select(.title + .body | test("#{upstream_number}|{upstream_number}"; "")) | {number, title, merged_at}]'

# Search open PRs
gh api "repos/openai/codex/pulls?state=open&per_page=100" \
  --jq '[.[] | select(.title + .body | test("#{upstream_number}|{upstream_number}"; "")) | {number, title}]'

# Also check if the upstream issue was closed
gh issue view -R openai/codex {upstream_number} --json state,closedAt,comments
```

**If a merged PR or closed upstream issue is found:**
1. Comment on the fork issue with the PR/closure reference
2. Close the fork issue
3. Stop execution — pick the next issue on next run

**If an open PR exists but is not merged:**
1. Comment on the fork issue noting the open PR
2. Continue only if the open PR is stale (no activity in 7+ days) or our approach differs

### 1d. Verify the issue is still reproducible

Read the upstream issue again to check for new comments that might invalidate it:

```bash
gh issue view -R openai/codex {upstream_number} --json body,comments,labels,state
```

If the issue was closed as "not a bug" or "won't fix", close the fork issue with a comment and stop.

---

## Phase 2: Reproduce the Bug

### 2a. Create a worktree

Use `EnterWorktree` to create an isolated workspace:

```
EnterWorktree(name: "fix-codex-{upstream_number}")
```

### 2b. Deep codebase analysis

Read the triage issue's "Potential Root Cause" and "Reproduction Plan" sections. Then:

1. **Locate the relevant source files** — use the hints from the triage issue and upstream discussion
2. **Read the code** around the suspected root cause
3. **Trace the data flow** from the entry point to the failure
4. **Check recent commits** that may have introduced or affected the area:
   ```bash
   git log --oneline -20 -- {relevant_files}
   ```
5. **Read existing tests** for the affected area to understand test patterns

### 2c. Write a reproduction test or script

Write a focused test or script that demonstrates the bug:

- **Prefer a test** (`#[test]` in Rust, or integration test) over a manual script
- The test should **fail on the current code** and **pass after the fix**
- If a test is impractical (requires external services, specific hardware), write a clear manual reproduction script with expected vs actual output

### 2d. Verify reproduction

Run the reproduction test/script and confirm it fails with the expected error:

```bash
cargo test -p {package} --test {test_file} -- {test_name}
```

### 2e. Critique the reproduction (adversarial subagents)

Spawn 2-3 subagents with **distinct analytical perspectives** to review the reproduction. Each agent must receive:
- The reproduction test/script code
- The upstream issue description
- The relevant source code around the bug

**Subagent 1 — "Skeptical Minimalist"**
> You are a skeptical reviewer who questions whether the reproduction actually captures the real bug. Your job is to find gaps: Does the test trigger the exact same code path as the real-world scenario? Are there environmental assumptions baked in? Could the test pass for the wrong reason? Could the test fail for a reason unrelated to the actual bug? Be ruthless about false positives and false negatives.

**Subagent 2 — "Edge Case Hunter"**
> You are an edge case specialist. Given a bug reproduction, you look for boundary conditions, race conditions, platform differences, and configuration variants that the reproduction might miss. Does the test cover the minimal case AND the complex case? What happens with empty inputs, unicode paths, concurrent access? Would this test catch a regression if someone "almost" fixed the bug but left a corner case?

**Subagent 3 (optional) — "Root Cause Verifier"**
> You are a root cause analyst. Your job is to verify that the reproduction test actually exercises the root cause identified in the triage issue, not just a symptom. Trace the code path from the test setup through to the failure point. Does the test isolate the root cause from confounding factors? If the root cause hypothesis is wrong, would this test still fail? Suggest how to make the test more diagnostic.

### 2f. Iterate on reproduction

Evaluate each subagent's feedback. For each piece of feedback:
1. Is it valid? (Does it point to a real gap?)
2. Is it actionable? (Can we improve the reproduction?)
3. Is it in scope? (Is it about THIS bug, not a different one?)

Update the reproduction test/script to address valid, actionable, in-scope feedback. Re-run to confirm it still demonstrates the bug.

**Repeat 2e-2f** until you have high confidence the reproduction is accurate. Usually 1-2 iterations suffice.

---

## Phase 3: Plan the Fix

### 3a. Draft an implementation plan

Spawn a subagent to draft the fix plan:

**Subagent — "Implementation Architect"**
> Given the reproduced bug, its root cause, and the relevant source code, draft a minimal implementation plan. The plan must:
> 1. List the exact files and functions to modify
> 2. Describe each change in concrete terms (not "refactor X" but "add a check for Y before calling Z")
> 3. Explain why this fix is correct (not just "it works" but "it's correct because...")
> 4. List potential side effects or regressions
> 5. Describe how to verify the fix (which tests to run, what to check)
> Consider multiple valid approaches and recommend the one that is simplest, most elegant, and has the smallest blast radius.

### 3b. Critique the plan (adversarial subagents)

Spawn 2-3 subagents with **distinct perspectives** to review the plan:

**Subagent 1 — "Minimalism Enforcer"**
> You enforce KISS and YAGNI. Review this fix plan and flag: unnecessary changes, over-engineering, changes that could be simpler, files touched that don't need to be, abstractions introduced where a direct fix suffices. The best fix is the smallest correct change. If the plan touches more than 3 files, justify each one.

**Subagent 2 — "Regression Guardian"**
> You protect against regressions. Review this fix plan and identify: existing tests that might break, behavior changes in unrelated features, performance implications, backwards compatibility concerns. Check if the fix handles all the cases the original code handled, not just the broken one.

**Subagent 3 (optional) — "Upstream Merge Strategist"**
> You think about upstream acceptance. Review this fix plan from the perspective of an OpenAI maintainer: Is the fix idiomatic for this codebase? Does it follow existing patterns? Would a maintainer accept this or prefer a different approach? Are there style, naming, or architectural conventions we should follow? Flag anything that looks "foreign" to the codebase.

### 3c. Iterate on the plan

Evaluate feedback, revise the plan, and re-critique if needed. Converge to a plan that:
- Makes the **minimum number of changes**
- Is **provably correct** (not just "works in testing")
- Follows **codebase conventions**
- Has **no unnecessary side effects**

---

## Phase 4: Implement and Validate

### 4a. Implement the fix

Apply the changes described in the finalized plan. Follow these rules:
- Change only what the plan specifies
- Don't refactor surrounding code
- Don't add comments unless the fix is non-obvious
- Don't change formatting or style of untouched code
- Match the codebase's existing patterns exactly

### 4b. Run the reproduction test

```bash
cargo test -p {package} --test {test_file} -- {test_name}
```

The previously-failing test must now pass.

### 4c. Run the full test suite for affected packages

```bash
cargo test -p {package}
```

No existing tests should break.

### 4d. Build check

```bash
cargo build --release 2>&1 | tail -20
cargo clippy -p {package} -- -D warnings 2>&1 | tail -20
```

No warnings or errors.

---

## Phase 5: Document and Finalize

### 5a. Commit the fix

Follow `git-commit-discipline`:

```bash
git add {changed_files}
git commit -m "$(cat <<'EOF'
fix({area}): {concise description}

{Why this change is needed — reference the upstream issue}

Root cause: {one-sentence root cause}
Fix: {one-sentence fix description}

Refs: openai/codex#{upstream_number}
EOF
)"
```

### 5b. Write a comprehensive issue comment

Add a comment to the fork issue with **full context** for a reviewer:

```bash
gh api repos/jeanibarz/codex/issues/{fork_number}/comments -X POST -f body="$(cat <<'EOF'
## Fix Implemented — {date}

### Root Cause Analysis
{Detailed explanation of what causes the bug, traced through the code}

### Reproduction
{How the bug was reproduced — test name, steps, expected vs actual output}

### Fix Description
{What was changed and why this is the correct minimal fix}

#### Files Changed
- `{file1}`: {what changed and why}
- `{file2}`: {what changed and why}

### Why This Fix Is Correct
{Reasoning about correctness — not just "tests pass" but why the fix addresses the root cause}

### Alternative Approaches Considered
{Other approaches that were evaluated and why they were rejected}

### Verification
- [ ] Reproduction test passes after fix
- [ ] Full package test suite passes
- [ ] `cargo clippy` clean
- [ ] No unrelated changes
- [ ] Fix follows codebase conventions

### Branch
`fix-codex-{upstream_number}` in worktree

### Upstream References
- Issue: openai/codex#{upstream_number}
- {Any related PRs or issues referenced in upstream discussion}
EOF
)"
```

### 5c. Add the ready-for-pr label

```bash
gh api repos/jeanibarz/codex/issues/{fork_number}/labels -X POST --input - <<'EOF'
{"labels":["ready-for-pr"]}
EOF
```

### 5d. Stop

Do NOT create a PR. The fix is ready for human review. The reviewer will:
1. Read the issue comment
2. Review the code in the worktree branch
3. Decide whether to create an upstream PR

---

## Anti-Patterns

- Don't start implementing before reproducing — reproduction proves you understand the bug
- Don't write the fix and reproduction test at the same time — write the test first, see it fail, then fix
- Don't ignore subagent feedback because it's inconvenient — address it or explain why it doesn't apply
- Don't touch code outside the bug's scope — no drive-by refactors, no style fixes, no "while I'm here"
- Don't commit untested code — every change must be verified
- Don't create the PR — that's the reviewer's decision
- Don't rush the critique phases — they prevent embarrassing mistakes

## Confidence Checklist

Before marking ready-for-pr, verify ALL of these:
- [ ] The reproduction test fails without the fix and passes with it
- [ ] The full test suite passes
- [ ] `cargo clippy` is clean
- [ ] The fix is the minimum correct change
- [ ] The issue comment explains everything a reviewer needs
- [ ] No unrelated changes were made
- [ ] The commit message references the upstream issue
