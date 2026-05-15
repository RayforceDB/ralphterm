# RalphTerm PTY Multiplexor Plan

> **For Hermes:** Use subagent-driven-development skill to implement this plan task-by-task.

**Goal:** Build a Rust service for autonomous planning, implementation, and review workflows while replacing brittle one-shot CLI automation with real interactive PTY sessions for Claude Code and Codex.

**Architecture:** `ralphterm` is a local daemon. It spawns one official CLI per session inside a real PTY, pastes prompts as user input, streams terminal output over WebSocket, and exposes REST controls for input, resize, cancel, transcript, and status. Higher-level workflow engines call this daemon instead of invoking AI CLIs directly.

**Tech Stack:** Rust 2021, tokio, axum, portable-pty, serde, uuid, dashmap. Tests use unit tests first, then API integration tests using harmless shell commands before real Claude/Codex smoke tests.

---

## Constraints

- Do not use `claude -p` or `--print` as the normal path.
- Do not call private provider APIs.
- Do not store provider credentials.
- Bind to `127.0.0.1` by default.
- Let the official CLI own login, identity, safety prompts, and rate limits.
- Make approvals explicit and auditable.
- Secrets stay in the user CLI/keychain/profile. The mux stores no provider credentials.

## Workflow Compatibility Targets

Mirror the executor contract required by autonomous engineering tools:

- Input: a long prompt string.
- Output: streamed text plus final transcript.
- Signals: `COMPLETED`, `FAILED`, `QUESTION`, `PLAN_READY`, `REVIEW_DONE`.
- Controls: cancellation, idle timeout, session timeout.
- Metadata: exit code, detected signal, recent text, transcript path.
- Adapter: a small CLI wrapper can translate `run(prompt)` into `POST /v1/sessions` + WebSocket streaming.

## Task 1: Keep the MVP compiling

**Objective:** Ensure the baseline daemon builds before adding features.

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/main.rs`
- Modify: `src/store.rs`
- Modify: `src/pty_agent.rs`
- Modify: `src/signals.rs`

**Steps:**
1. Run `cargo fmt --all -- --check`.
2. Run `cargo clippy --all-targets --all-features -- -D warnings`.
3. Run `cargo test --all`.
4. Fix only compile/lint/test failures.
5. Commit: `chore: keep mvp build green`.

## Task 2: Add deterministic shell-agent API tests

**Objective:** Prove session lifecycle without real Claude/Codex dependencies.

**Files:**
- Create: `tests/session_api.rs`

**Test fixture:**

```bash
/bin/sh -lc 'read line; printf "%s\n" "$line"; echo COMPLETED'
```

**Assertions:**
- `POST /v1/sessions` returns an id.
- status eventually becomes `exited`.
- signal is `COMPLETED`.
- exit code is `0`.
- transcript contains the prompt and completion marker.

## Task 3: Implement real PTY resize

**Objective:** Wire `POST /resize` to `portable-pty` resize instead of consuming placeholder messages.

**Files:**
- Modify: `src/store.rs`

**Steps:**
1. Store a resize-capable PTY handle in `SessionHandle`.
2. Update `SessionStore::resize` to call `resize(PtySize { rows, cols, ... })`.
3. Add a unit-level test around request validation.
4. Commit: `feat: resize live pty sessions`.

## Task 4: Add idle/session timeouts

**Objective:** Protect operators from hung CLIs.

**Files:**
- Modify: `src/pty_agent.rs`
- Modify: `src/store.rs`

**Steps:**
1. Add optional `idle_timeout_secs` and `max_runtime_secs` to `SessionConfig`.
2. Track last output timestamp.
3. Kill and mark timed out when limits are exceeded.
4. Emit timeout events.
5. Commit: `feat: enforce session and idle timeouts`.

## Task 5: Add generic executor adapter

**Objective:** Let external workflow tools use RalphTerm as a drop-in execution backend.

**Files:**
- Create: `adapters/ralphterm-exec/README.md`
- Create: `adapters/ralphterm-exec/ralphterm-exec.sh`

**Steps:**
1. Script reads prompt from stdin.
2. Script posts to local RalphTerm daemon.
3. Script streams output to stdout.
4. Script exits non-zero if the mux session fails or times out.
5. Document configuration for any workflow engine that supports custom commands.
6. Commit: `feat: add generic executor adapter`.

## Task 6: Add approval policy hooks

**Objective:** Detect approval prompts and surface them safely.

**Files:**
- Create: `src/approvals.rs`
- Modify: `src/store.rs`
- Modify: `src/signals.rs`

**Steps:**
1. Add `ApprovalRequest` struct.
2. Detect common prompt phrases in output.
3. Emit `approval-requested` event.
4. Add manual approval input endpoint.
5. Add a strict allowlist-based auto policy only after manual mode works.
6. Commit: `feat: add approval request events`.

## Task 7: Add opt-in real CLI smoke tests

**Objective:** Verify real Claude/Codex PTY behavior without making CI depend on logged-in CLIs.

**Files:**
- Create: `scripts/smoke-claude.sh`
- Create: `scripts/smoke-codex.sh`

**Steps:**
1. Check `command -v claude` or `command -v codex`.
2. Check auth status if available.
3. Start a short local session.
4. Require user opt-in env var, e.g. `RALPHTERM_SMOKE_REAL_CLI=1`.
5. Never run real CLI smoke tests in default CI.
6. Commit: `test: add opt-in real cli smoke tests`.

## Task 8: Feature parity roadmap

**Objective:** Document the workflow features RalphTerm must own directly.

**Files:**
- Create: `docs/milestones/m1-autonomous-engineering.md`

**Content:**
- task intake
- planning phase
- implementation phase
- review phase
- dashboard
- notifications
- workspaces
- persistence
- safety controls

**Commit:** `docs: add autonomous workflow milestone`
