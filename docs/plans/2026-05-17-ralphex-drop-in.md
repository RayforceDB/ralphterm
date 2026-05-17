# Ralphex Drop-in Replacement Implementation Plan

> **For Hetoku:** Use subagent-driven-development skill to implement this plan task-by-task. Do not push every checkpoint. Batch local commits and push only coherent release slices.

**Goal:** Make RalphTerm usable as a direct drop-in replacement for ralphex while preserving the core ralphex cross-review verification semantics.

**Architecture:** Add a ralphex-compatible CLI/config/front-door on top of RalphTerm's PTY runner, then grow the execution pipeline from task-only into full ralphex parity: task execution, review loops, external review, evaluator/fixer loop, final review, progress logs, worktrees, dashboard, notifications, Docker isolation, alternate-provider wrappers. Keep the native `ralphterm run/serve` API, but make `ralphterm [ralphex flags] <plan.md>` and eventually a `ralphex` binary alias work the same way existing ralphex users expect. Rebuild the website + docs around the drop-in story.

**Tech Stack:** Rust, clap, portable-pty, git CLI, serde/JSON config, integration tests with temporary git repos and fake agent/reviewer scripts.

---

## Compatibility Source of Truth

Inspected local ralphex repo at `/home/hetoku/work/ralphex`.

Key files:

- `cmd/ralphex/main.go`
- `pkg/config/config.go`
- `pkg/processor/runner.go`
- `pkg/config/defaults/prompts/task.txt`
- `pkg/config/defaults/prompts/review_first.txt`
- `pkg/config/defaults/prompts/review_second.txt`
- `pkg/config/defaults/prompts/codex_review.txt`
- `pkg/config/defaults/prompts/codex.txt`
- `CLAUDE.md`

Ralphex CLI surface to match:

```text
ralphex [flags] [plan-file]

-m, --max-iterations
--max-external-iterations
--review-patience
--task-model
--review-model
--claude-command
--claude-args
--external-review-tool=codex|custom|none
--custom-review-script
-r, --review
-e, --external-only
-c, --codex-only
-t, --tasks-only
-b, --base-ref
--wait
--session-timeout
--idle-timeout
--skip-finalize
--preserve-anthropic-api-key
--worktree
--branch
--plan
-d, --debug
--no-color
-v, --version
-s, --serve
-p, --port
--host
-w, --watch
--init
--reset
--dump-defaults
--config-dir / RALPHEX_CONFIG_DIR
```

Ralphex config surface to match:

- global config: `~/.config/ralphex/`
- local project config: `.ralphex/`
- default prompts and agents
- `claude_command`, `claude_args`
- task/review model split
- codex/external review config
- timeouts/retry/wait settings
- worktree, plans dir, watch dirs, default branch
- move plan on completion
- notify settings

Core semantic to preserve:

```text
IMPLEMENTATION COMPLETED is not acceptance.
Acceptance requires validation plus independent review loops until reviewers find zero actionable issues.
```

---

## Acceptance Criteria

- A user can replace `ralphex` with `ralphterm` for common plan execution: `ralphterm --tasks-only --claude-command <cmd> docs/plans/foo.md`.
- A `ralphex` binary alias is produced by the Rust build and runs the same compatibility parser.
- `ralphterm [flags] <plan-file>` supports the major ralphex modes without requiring the `run` subcommand.
- `--claude-command` drives a PTY command, never `claude -p` / `--print`.
- `--external-review-tool=custom --custom-review-script <cmd>` maps to RalphTerm's independent review command.
- Full mode default is task execution plus review gate; `--tasks-only` skips review intentionally.
- Review gates run independently and can block `[x]` marking and local checkpoint commits.
- Local checkpoint commits are allowed, but pushes are batched into release slices.
- Compatibility behavior is covered by fake-agent/fake-reviewer E2E tests.
- Notifications, Docker, and alternate-provider wrappers each have at least one integration test.
- The website ships a dedicated ralphex-compatibility story, migration guide, and CLI reference with assertions in `tests/site_copy.rs`.

---

### Task 1: Add no-subcommand ralphex-style CLI entry point

**Objective:** Allow `ralphterm --tasks-only --claude-command <cmd> <plan.md>` to run a plan without the `run` subcommand.

**Files:**
- Modify: `src/main.rs`
- Modify: `tests/run_plan_fake_agent.rs`

**Step 1: Write failing test**

Add integration test:

```rust
#[test]
fn ralphex_style_cli_runs_plan_without_run_subcommand() {
    // temp git repo with plan.md
    // command: ralphterm --tasks-only --claude-command tests/fixtures/fake-agent.sh --no-commit plan.md
    // expect: success, first.txt exists, output contains Executing plan.md
}
```

**Step 2: Verify RED**

Run:

```bash
cargo test --test run_plan_fake_agent ralphex_style_cli_runs_plan_without_run_subcommand -- --nocapture
```

Expected: FAIL with clap rejecting `--tasks-only` because current CLI requires a subcommand.

**Step 3: Implement minimal parser**

Change `Cli.command` to `Option<Command>` and add top-level compatibility args:

- `--tasks-only`
- `--claude-command`
- `--custom-review-script`
- `--external-review-tool=custom|none`
- `--no-commit`
- positional `plan-file`

Route `None` command into the same `run_plan` path.

**Step 4: Verify GREEN**

Run the same test. Expected: PASS.

---

### Task 2: Add `ralphex` binary alias

**Objective:** `cargo build` should produce both `ralphterm` and `ralphex`, so scripts can point at the new binary name without shell wrappers.

**Files:**
- Modify: `Cargo.toml`
- Create: `src/bin/ralphex.rs` or refactor shared entry into `src/cli.rs`
- Modify: `src/main.rs`
- Test: `tests/ralphex_alias.rs`

**Step 1: Write failing test**

Integration test invokes:

```rust
Command::new(env!("CARGO_BIN_EXE_ralphex"))
    .args(["--tasks-only", "--claude-command", fake_agent, "--no-commit", plan])
```

Expected first: compile/test fails because no `ralphex` binary exists.

**Step 2: Refactor main**

Move current CLI entry into `ralphterm::cli::run()` or `src/cli.rs`, then make both binaries call it.

**Step 3: Verify**

```bash
cargo test --test ralphex_alias -- --nocapture
cargo test --all
```

---

### Task 3: Map core ralphex flags to RunOptions

**Objective:** Accept ralphex's most used flags even when some are initially no-op with explicit warnings.

**Files:**
- Modify: `src/main.rs`
- Modify: `tests/run_plan_fake_agent.rs`

**Flags to support now:**

- `--tasks-only`
- `--review`
- `--external-only` / `--codex-only`
- `--max-iterations`
- `--max-external-iterations`
- `--review-patience`
- `--claude-command`
- `--claude-args`
- `--external-review-tool`
- `--custom-review-script`
- `--base-ref`
- `--session-timeout`
- `--idle-timeout`
- `--wait`
- `--debug`
- `--no-color`
- `--version`

**Step 1: Test accepted flags**

Create tests that run `--help` and a smoke dry path with these flags.

**Step 2: Wire behavior**

- `--claude-command + --claude-args` joins into implementation command.
- `--external-review-tool=custom` requires `--custom-review-script` and maps to review command.
- `--external-review-tool=none` disables review.
- unsupported-but-accepted flags print warning only when relevant.

---

### Task 4: Make default full mode review-gated

**Objective:** Ralphex full mode should not default to task-only behavior.

**Files:**
- Modify: `src/main.rs`
- Modify: `src/runner.rs`
- Modify: `tests/run_plan_fake_agent.rs`
- Fixtures: `tests/fixtures/review-pass.sh`, `review-fail.sh`

**Behavior:**

- No `--tasks-only`: run task phase plus review gate.
- `--tasks-only`: skip reviews.
- `--external-review-tool=custom --custom-review-script <cmd>` supplies the review command.
- Full mode without available review tool should fail with a clear message, not silently claim acceptance.

**Tests:**

- default/full with pass reviewer marks `[x]`.
- default/full with fail reviewer exits non-zero and leaves `[ ]`.
- tasks-only succeeds without reviewer and prints that review was skipped by mode.

---

### Task 5: Add ralphex config loader compatibility

**Objective:** Load `~/.config/ralphex/` and `.ralphex/` enough to preserve existing user setup.

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs`
- Tests: `tests/config_compat.rs`

**Behavior:**

- Default config dir: `~/.config/ralphex/`.
- `--config-dir` and `RALPHEX_CONFIG_DIR` override it.
- `.ralphex/config.json` in cwd overrides global config per field.
- At minimum parse command/config values used by Tasks 1-4.

---

### Task 6: Port ralphex signal semantics

**Objective:** Match completion/failure/review protocol exactly enough for prompts and fake agents to be reusable.

**Files:**
- Modify: `src/runner.rs`
- Create/modify: `src/signals.rs`
- Tests: `tests/signals_compat.rs`

**Signals:**

- `<<<RALPHEX:ALL_TASKS_DONE>>>`
- `<<<RALPHEX:TASK_FAILED>>>`
- `<<<RALPHEX:REVIEW_DONE>>>`
- `<<<RALPHEX:CODEX_REVIEW_DONE>>>`
- current RalphTerm `COMPLETED`, `FAILED`, `REVIEW_PASS`, `REVIEW_FAIL` as compatibility aliases

**Rule:** `REVIEW_DONE` means this iteration found zero issues, not that fixes are accepted.

---

### Task 7: Implement review retry loop with fixer re-run

**Objective:** If review fails, re-run implementation/fixer and then re-review. Only accept after a clean review iteration.

**Files:**
- Modify: `src/runner.rs`
- Tests: `tests/run_plan_fake_agent.rs`
- Fixtures: `retry-after-review-agent.sh`, `review-fail-once.sh`

**Behavior:**

```text
implement attempt 1
validation pass
review fail
restore/keep controlled diff state
implement/fix attempt 2 with review context
validation pass
review pass
mark [x]
local commit
```

---

### Task 8: Add external-only / review-only modes

**Objective:** Support ralphex operational modes for existing workflows.

**Files:**
- Modify: `src/main.rs`
- Modify: `src/runner.rs`
- Tests: `tests/mode_compat.rs`

**Modes:**

- `--review`: skip task execution, run full review pipeline.
- `--external-only` / `--codex-only`: run external review/fixer loop only.
- `--tasks-only`: current task execution path.

---

### Task 9: Worktree and branch compatibility

**Objective:** Support `--worktree --branch` enough for safe isolated runs.

**Files:**
- Modify: `src/workspace.rs`
- Modify: `src/main.rs`
- Tests: `tests/worktree_compat.rs`

**Behavior:**

- `--worktree` creates isolated git worktree.
- branch defaults to plan filename slug.
- `--branch` overrides.
- plan path is resolved inside worktree.

---

### Task 10: Progress/dashboard compatibility layer

**Objective:** Ralphex users can still observe runs and tail progress files.

**Files:**
- Modify: `src/runs.rs`
- Modify: `src/runner.rs`
- Modify: `src/main.rs`
- Tests: `tests/progress_compat.rs`

**Behavior:**

- Progress files in `.ralphex/progress/` or compatible symlink/index from `.ralphterm/progress/`.
- `--serve --port --host --watch` works with ralphex-like flags.
- Watch-only mode works when no plan is supplied.

---

### Task 11: Plan completion movement

**Objective:** Match ralphex's completed-plan behavior.

**Files:**
- Modify: `src/runner.rs`
- Modify: `src/plan.rs`
- Tests: `tests/plan_completion_compat.rs`

**Behavior:**

- On successful full/tasks mode, move plan to `docs/plans/completed/` if enabled.
- Preserve ralphex date-format rename tolerance later.
- Respect config `move_plan_on_completion`.

---

### Task 12: Notifications

**Objective:** Match ralphex notification surface so existing automation hooks keep working.

**Files:**
- Create: `src/notify.rs`
- Modify: `src/runner.rs`, `src/config.rs`, `src/main.rs`
- Tests: `tests/notify_compat.rs`

**Behavior:**

- Channels: Telegram, Slack, Email (SMTP), Webhook (generic POST JSON).
- Config keys under `[notify]`: `telegram_token`, `telegram_chat_id`, `slack_webhook`, `email_smtp`, `email_to`, `webhook_url`, `notify_on=plan_done|task_failed|review_failed|rate_limit`.
- CLI overrides: `--notify-telegram-token`, `--notify-telegram-chat`, `--notify-slack`, `--notify-webhook`, `--notify-on a,b,c`.
- Notifications fire on plan completion, plan failure, repeated review failure, and detected rate-limit pauses.
- Each delivery has a 10 s timeout and never blocks the run.
- Fake HTTP server fixture in tests verifies payloads.

---

### Task 13: Docker isolation

**Objective:** Match ralphex's Docker-isolated execution so users can run untrusted plans without trusting the host shell.

**Files:**
- Create: `docker/Dockerfile`, `docker/entrypoint.sh`
- Create: `src/docker.rs`
- Modify: `src/runner.rs`, `src/main.rs`, `src/config.rs`
- Tests: `tests/docker_compat.rs` (skipped when `docker` binary not present)

**Behavior:**

- `--docker` runs the plan agent (and reviewer) inside a container.
- Image defaults to `ralphterm:latest`; override with `--docker-image`.
- Bind-mounts workspace, plan file, and `~/.claude` (or configurable auth dir) read-write; `~/.config/ralphterm/` read-only.
- Honors `RALPHEX_EXTRA_VOLUMES`, `RALPHEX_EXTRA_ENV`, `TZ`, `AWS_PROFILE`, `AWS_REGION`.
- `--preserve-anthropic-api-key` passes through `ANTHROPIC_API_KEY` env var.
- Tests use a stub Docker harness when binary missing, otherwise spin a tiny ubuntu image.

---

### Task 14: Alternate-provider wrapper scripts

**Objective:** Ralphex supports Copilot, Codex, Gemini, and OpenCode via wrapper scripts. RalphTerm should ship the same wrappers so plans authored for ralphex run unchanged.

**Files:**
- Create: `scripts/wrappers/copilot.sh`, `codex.sh`, `gemini.sh`, `opencode.sh`
- Create: `docs/providers.md`
- Modify: `src/config.rs` (auto-detect via `agent=copilot|codex|gemini|opencode`)
- Tests: `tests/wrappers_compat.rs` (uses fake child binaries)

**Behavior:**

- `--agent codex|copilot|gemini|opencode` picks a wrapper script that translates RalphTerm's PTY input/signals into provider-specific behavior.
- Wrappers live in `scripts/wrappers/` and are installed alongside the binary.
- `claude_command` still wins when set explicitly.

---

### Task 15: Website rebuild around the ralphex drop-in story

**Objective:** Land the marketing/docs story for "drop-in ralphex replacement with PTY execution".

**Files:**
- Rewrite: `site/index.html`
- New pages: `site/docs/ralphex-compat.html`, `site/docs/cli.html`, `site/docs/migrate-from-ralphex.html`, `site/docs/providers.html`, `site/docs/notifications.html`, `site/docs/docker.html`
- Update: `site/docs/index.html` (nav), `site/assets/styles.css` (new components if needed)
- New assets: `site/assets/comparison-table.svg` or HTML, hero terminal recording (asciicast or animated SVG)
- Tests: extend `tests/site_copy.rs` to assert key claims and links

**Copy + structure:**

- Hero rewrite: `Drop-in ralphex replacement. Real PTY, real CLIs, real review gates.`
- Side-by-side comparison: `ralphex --tasks-only plan.md` vs `ralphterm --tasks-only plan.md`.
- Three-card section: same flags, same plan format, harder review gate.
- Migration page: step-by-step move from ralphex (config dir, prompts, agents, env vars).
- CLI reference: every supported flag with ralphex-compat notes.
- Notifications, Docker, providers pages document the new surfaces from tasks 12-14.
- Update meta tags, OG image alt text, sitemap.xml.

---

### Task 16: Documentation parity

**Objective:** Make in-repo docs match the new site and cover every ralphex surface.

**Files:**
- Update: `README.md`, `docs/getting-started.md`, `docs/workflows.md`
- New: `docs/ralphex-compat.md`, `docs/cli-reference.md`, `docs/migrate-from-ralphex.md`, `docs/notifications.md`, `docs/docker.md`, `docs/providers.md`
- Update: `docs/architecture.md` (notification + docker + wrapper layers)
- Tests: `tests/docs_links.rs` (basic broken-link check)

**Constraints:**

- README leads with the drop-in pitch and a one-liner replacement example.
- Every CLI flag from `--help` appears in `docs/cli-reference.md`.
- Migration guide is verified against a real `.ralphex/` directory in CI fixture.

---

## Verification Gates

Before any release push:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```

Plus compatibility smoke:

```bash
cargo build
./target/debug/ralphterm --tasks-only --claude-command tests/fixtures/fake-agent.sh --no-commit /tmp/plan.md
./target/debug/ralphex --tasks-only --claude-command tests/fixtures/fake-agent.sh --no-commit /tmp/plan.md
```

Do not push until the release slice has a clear changelog and all local checks pass.
