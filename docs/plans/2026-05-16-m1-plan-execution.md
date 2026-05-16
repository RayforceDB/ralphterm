# M1 Plan Execution Implementation Plan

> **For Hetoku:** This is the real product goal. RalphTerm must become a replacement-grade autonomous plan runner, not just a pretty PTY demo.

**Goal:** Given a markdown plan, RalphTerm executes tasks one by one through real terminal-backed AI CLI sessions, validates results, runs an independent cross-review gate, commits accepted progress locally, and produces an auditable transcript.

**Current truth:** RalphTerm can parse a markdown plan, execute pending tasks through fresh PTY-backed agent sessions, run validation commands, run an independent reviewer after validation through `--review-agent` or `--review-command`, retry implementation once with reviewer feedback, mark completed checkboxes, commit task progress, and write transcripts under `.ralphterm/progress/`. It is still not full ralphex parity: the review loop is bounded and local rather than a full workspace-isolated approval queue. The invariant is already in place for reviewed runs, though: a task is not accepted merely because the implementation agent prints `COMPLETED`; acceptance requires validation and any configured/required review must print `REVIEW_PASS` before `[x]` and checkpoint.

**Architecture:** Keep the PTY session layer as the foundation. Add a CLI subcommand and library modules for plan parsing, task execution, validation commands, git commits, and run logs. Build with strict TDD: parser first, then dry-run planner, then real task execution with a fake agent command, then Claude/Codex smoke tests.

**Tech Stack:** Rust, axum server, portable-pty, git CLI, markdown parser or small custom parser, integration tests using temporary git repos and fake agent shell scripts.

---

## Acceptance Criteria

- `ralphterm run docs/plans/my-feature.md` works from a git repository root.
- Plan parser detects validation commands and incomplete `### Task N:` sections with `- [ ]` checkboxes.
- Each task starts a fresh PTY-backed agent session.
- RalphTerm sends task context to the agent as terminal input, not `claude -p`.
- Validation commands run after each task.
- An independent reviewer runs after validation and before task acceptance. **Shipped:** `--review-agent codex` uses a built-in reviewer CLI; `--review-command <cmd>` starts a custom PTY reviewer after validation. It receives the task, agent transcript, validation output, and git state. Exact `REVIEW_PASS` accepts. Exact `REVIEW_FAIL` retries implementation with reviewer feedback while the configured retry budget allows it; the default budget is one retry, and `--max-review-retries N` can raise, lower, or disable it.
- Completed tasks are marked `[x]` only after validation and review both pass.
- A git commit is created after each accepted task, kept local until a coherent release slice is ready.
- A progress log/transcript is written under `.ralphterm/progress/`.
- A fake-agent + fake-reviewer integration test proves the whole loop without spending tokens.
- A real Claude/Codex smoke test is documented and can be run manually when auth is present.

---

### Task 1: Add plan parser tests

**Objective:** Define the exact plan format RalphTerm supports.

**Files:**
- Create: `tests/plan_parser.rs`
- Create/Modify: `src/plan.rs`
- Modify: `src/main.rs`

**Test cases:**
- Parses `## Validation Commands` command bullets.
- Parses incomplete task sections named `### Task N: title`.
- Ignores completed `[x]` checkboxes.
- Preserves task body text for agent prompt construction.

**Run:**

```bash
cargo test --test plan_parser
```

Expected first: fail because `src/plan.rs` does not exist.

---

### Task 2: Implement minimal parser

**Objective:** Make parser tests pass with a small custom parser.

**Files:**
- Create: `src/plan.rs`
- Modify: `src/main.rs`

**Rules:**
- No broad markdown framework unless needed.
- Keep `Task` keyword in English.
- Only checkbox lines under task sections count as task work.

**Run:**

```bash
cargo test --test plan_parser
cargo test --all
```

---

### Task 3: Add fake-agent integration test

**Objective:** Prove the loop can drive an interactive command through PTY without real Claude/Codex.

**Files:**
- Create: `tests/run_plan_fake_agent.rs`
- Create: `tests/fixtures/fake-agent.sh`

**Fake agent behavior:**
- Reads task prompt from stdin.
- Writes a file requested by the task.
- Prints `COMPLETED`.
- Exits 0.

**Run:**

```bash
cargo test --test run_plan_fake_agent
```

Expected first: fail because `ralphterm run` does not exist.

---

### Task 4: Add `ralphterm run <plan>` CLI skeleton

**Objective:** Add command routing and dry-run output.

**Files:**
- Modify: `src/main.rs`
- Create: `src/runner.rs`

**Behavior:**
- `ralphterm serve` remains unchanged.
- `ralphterm run <plan.md>` parses the plan and prints pending task titles in order.
- `--agent-command <cmd>` lets tests inject fake agent.

**Run:**

```bash
cargo test --test run_plan_fake_agent
cargo test --all
```

---

### Task 5: Execute one task through PTY

**Objective:** Reuse existing PTY session machinery for a single task.

**Files:**
- Modify: `src/runner.rs`
- Modify: `src/store.rs` if library extraction is needed

**Behavior:**
- Starts a fresh command per task.
- Sends task prompt via PTY stdin.
- Waits for `COMPLETED` or process exit.
- Captures transcript.

**Run:**

```bash
cargo test --test run_plan_fake_agent
```

---

### Task 6: Run validation commands

**Objective:** Validation gates task completion.

**Files:**
- Modify: `src/runner.rs`
- Modify: `tests/run_plan_fake_agent.rs`

**Behavior:**
- Run commands from `## Validation Commands` after task completes.
- If validation fails, do not mark task done and exit non-zero.

---

### Task 7: Add independent cross-review gate

**Objective:** Match the core ralphex value: task work is not accepted until an independent reviewer verifies it.

**Files:**
- Modify: `src/main.rs`
- Modify: `src/runner.rs`
- Modify: `tests/run_plan_fake_agent.rs`
- Create: `tests/fixtures/review-pass.sh`
- Create: `tests/fixtures/review-fail.sh`

**Behavior:**
- Add `--review-command <cmd>`.
- After agent completion and validation, run review command in a fresh PTY.
- Review prompt includes plan task, implementation transcript, validation output, and current `git diff`.
- `REVIEW_PASS` allows acceptance.
- `REVIEW_FAIL` rejects the current attempt; a second failed review blocks `[x]`, commit, and returns non-zero.
- If no reviewer is configured, print that review is skipped; do not claim ralphex parity.

**Tests:**
- pass reviewer allows marking and commit
- fail reviewer blocks marking and commit

---

### Task 8: Mark tasks complete and commit

**Objective:** Match the practical workflow: task accepted means validated, reviewed, checked, and locally committed.

**Files:**
- Modify: `src/plan.rs`
- Modify: `src/runner.rs`
- Modify: `tests/run_plan_fake_agent.rs`

**Behavior:**
- Change completed task checkboxes from `[ ]` to `[x]` only after validation and review pass.
- `git add` relevant changes.
- `git commit -m "task: <task title>"` locally.
- Do not push per-task commits; batch pushes for release slices.
- Skip commit only with explicit `--no-commit`.

---

### Task 9: Progress logs and transcripts

**Objective:** Make long runs observable and restartable later.

**Files:**
- Modify: `src/runner.rs`
- Create: `src/progress.rs`

**Behavior:**
- Write `.ralphterm/progress/<plan-slug>.log`.
- Include timestamps, task start/end, validation result, commit hash, signal, and transcript path.

---

### Task 10: Real CLI smoke test documentation

**Objective:** Document the manual smoke test for Claude/Codex without requiring tokens in CI.

**Files:**
- Modify: `docs/getting-started.md`
- Modify: `README.md`

**Behavior:**
- Show `ralphterm run docs/plans/example.md --agent claude`.
- Explain that official CLI auth must already work.
- Explicitly state RalphTerm does not use `claude -p`.

---

### Task 11: Website reposition after implementation exists

**Objective:** Public site should lead with the real goal once M1 works.

**Files:**
- Modify: `site/index.html`
- Modify: `site/assets/social-preview.png`

**Copy direction:**
- `Write a plan. Let real terminal agents execute it.`
- Mention PTY/API as implementation detail, not the product purpose.

---
