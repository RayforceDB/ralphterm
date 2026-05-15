# PTY Multiplexor Implementation Plan

> **For Hermes:** Use subagent-driven-development skill to implement this plan task-by-task.

**Goal:** Build a Rust service that preserves ralphex-style autonomous planning/review workflows while replacing `claude -p`/one-shot CLI automation with real interactive PTY sessions for Claude Code and Codex.

**Architecture:** `ralphex-mux` is a local daemon. It spawns one official CLI per session inside a real PTY, pastes prompts as user input, streams terminal output over WebSocket, and exposes REST controls for input, resize, cancel, transcript, and status. Ralphex can later call this daemon as an executor backend instead of invoking `claude --print` directly.

**Tech Stack:** Rust 2021, tokio, axum, portable-pty, serde, uuid, dashmap. Tests use unit tests first, then API integration tests using harmless shell commands before real Claude/Codex smoke tests.

---

## Constraints

- Do not call provider private APIs.
- Do not emulate auth or bypass product limits.
- Do not depend on `claude -p`/`--print` for normal operation.
- Use official local CLIs exactly as a user would: `claude`, `codex`, or user-provided command.
- Approval automation must be explicit policy, visible in logs, and conservative by default.
- Secrets stay in the user CLI/keychain/profile. The mux stores no provider credentials.

## Ralphex Compatibility Targets

Mirror the current ralphex executor contract:

- Input: a long prompt string.
- Output: streamed text chunks plus final transcript.
- Signals: `COMPLETED`, `ALL_TASKS_DONE`, `FAILED`, `QUESTION`, `PLAN_READY`, `REVIEW_DONE`.
- Controls: cancellation, idle timeout, session timeout.
- Metadata: exit code, detected signal, recent text, transcript path.
- Future adapter: a small `ralphex` executor wrapper can translate `Executor.Run(ctx, prompt)` into `POST /v1/sessions` + WebSocket streaming.

## Task 1: Keep the MVP compiling

**Objective:** Preserve the current checked-in Rust skeleton and keep `cargo check` green.

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/main.rs`
- Modify: `src/store.rs`
- Modify: `src/pty_agent.rs`
- Modify: `src/signals.rs`

**Steps:**
1. Run `~/.cargo/bin/cargo check`.
2. Fix any compile errors.
3. Run `~/.cargo/bin/cargo fmt`.
4. Run `~/.cargo/bin/cargo test`.
5. Commit: `feat: add pty mux api skeleton`.

## Task 2: Add shell-command test agent

**Objective:** Allow deterministic tests without Claude/Codex by creating sessions with `command=/bin/sh` and args.

**Files:**
- Modify: `src/pty_agent.rs`
- Modify: `src/store.rs`
- Create: `tests/api_shell.rs`

**Steps:**
1. Add an integration test that starts the server on an ephemeral port.
2. `POST /v1/sessions` with `agent=claude`, `command=/bin/sh`, `args=["-lc", "cat; echo COMPLETED"]`, prompt `hello`.
3. Subscribe to `/events` and assert output contains `hello` and signal becomes `COMPLETED`.
4. Assert `/transcript` returns the whole PTY transcript.
5. Commit: `test: cover pty session lifecycle with shell agent`.

## Task 3: Fix PTY resize properly

**Objective:** Wire `/resize` to the active PTY master instead of the current accepted/no-op placeholder.

**Files:**
- Modify: `src/store.rs`

**Steps:**
1. Refactor session internals so the PTY master can be shared safely for resize while reader/writer handles remain active.
2. Add a unit/integration test using `stty size` to prove resize affects the child PTY.
3. Commit: `feat: implement live pty resize`.

## Task 4: Add idle/session timeouts

**Objective:** Match ralphex safety behavior for hung CLIs.

**Files:**
- Modify: `src/main.rs`
- Modify: `src/store.rs`
- Modify: `src/pty_agent.rs`

**Steps:**
1. Add `idle_timeout_ms` and `session_timeout_ms` to `CreateSessionRequest`.
2. Reset idle deadline on every output chunk.
3. Kill the child process on timeout and emit `SessionEvent::Error` with a typed reason.
4. Test with `/bin/sh -lc 'sleep 60'` and short timeout.
5. Commit: `feat: enforce session and idle timeouts`.

## Task 5: Add ralphex executor adapter

**Objective:** Let ralphex use this mux as a drop-in execution backend.

**Files:**
- Create: `adapters/ralphex-mux-exec/README.md`
- Create: `adapters/ralphex-mux-exec/ralphex-mux-exec.sh`
- Later modify upstream ralphex `pkg/executor` only after adapter works.

**Steps:**
1. Script reads prompt from stdin.
2. Script creates a mux session.
3. Script streams WebSocket output to stdout.
4. Script exits non-zero if mux session exits failed/timed out.
5. Configure ralphex with `claude_command = /path/to/ralphex-mux-exec.sh` and empty args.
6. Commit: `feat: add ralphex executor adapter`.

## Task 6: Add approval policy hooks

**Objective:** Support real-user workflows without silent unsafe bypasses.

**Files:**
- Create: `src/approval.rs`
- Modify: `src/store.rs`
- Modify: `src/main.rs`

**Steps:**
1. Define policy enum: `manual`, `allow-readonly`, `allow-configured-patterns`.
2. Detect common Claude/Codex approval prompts in PTY output.
3. Emit `approval-requested` event with prompt text.
4. Only send approval keys if the policy explicitly matches.
5. Log every auto-approval event in transcript metadata.
6. Commit: `feat: add explicit approval policy hooks`.

## Task 7: Real CLI smoke tests

**Objective:** Verify the mux works with installed Claude/Codex without relying on private internals.

**Files:**
- Create: `scripts/smoke-claude.sh`
- Create: `scripts/smoke-codex.sh`

**Steps:**
1. Check `command -v claude` and `command -v codex`.
2. Start mux locally.
3. Run harmless prompt: `Reply with COMPLETED only.`
4. Verify signal detection.
5. Do not run in CI unless credentials are present and user opts in.
6. Commit: `test: add opt-in real cli smoke tests`.

## Task 8: Ralphex feature parity roadmap

**Objective:** Document parity against ralphex features so users can keep evaluating the workflow.

**Files:**
- Create: `docs/ralphex-compat.md`

**Content:**
- Task execution loop: adapter target.
- Review phases: supported through repeated mux sessions.
- Plan creation: supported through interactive session plus QUESTION/PLAN_READY signals.
- Web dashboard: mux API can feed ralphex dashboard or its own UI.
- Notifications: remain in ralphex initially.
- Worktrees/git: remain in ralphex initially.
- Docker isolation: future.

**Commit:** `docs: add ralphex compatibility roadmap`.
