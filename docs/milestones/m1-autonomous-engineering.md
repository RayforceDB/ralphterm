# Milestone 1: Autonomous Engineering Loop

> **For Hermes:** Use subagent-driven-development skill to implement this plan task-by-task.

**Goal:** Ship RalphTerm as a complete local autonomous engineering workflow powered by real interactive PTY sessions.

**Architecture:** RalphTerm remains a local daemon. The first milestone adds the workflow layer above the current PTY/session API: run intake, workspace setup, implementation, validation, independent review, dashboard, notifications, and persistent run history.

**Tech Stack:** Rust 2021, axum, tokio, portable-pty, serde, uuid, static HTML/CSS/JS for the first dashboard, file-backed JSONL storage for run history.

---

## Acceptance criteria

A user can run one command, submit an engineering task against a repository, watch Claude/Codex work in real PTY sessions, see review status, inspect transcripts, and receive a final result with patches and audit trail.

## Feature set

### 1. Run intake

- CLI: `ralphterm run --repo /path/to/repo --agent claude --task task.md`
- API: `POST /v1/runs`
- Dashboard: form with repo path, agent, task, and policy

### 2. Workspace isolation

- create a per-run worktree or copy strategy
- record base commit and branch name
- never mutate the source repo without explicit mode
- expose workspace path in status

### 3. Execution phases

- `planning`
- `implementation`
- `validation`
- `independent-review`
- `finalize`

Each phase is a PTY session with its own transcript and signals.

```text
implement -> validate -> independent-review -> accept/commit
```

### 4. Review loop

- run a second agent or second session as the independent reviewer
- feed diff and implementation transcript
- require explicit `REVIEW_PASS` before acceptance
- treat `REVIEW_FAIL` as retry feedback while the retry budget allows it
- optionally send findings back to implementation agent

### 5. Dashboard

- active run list
- terminal stream
- phase timeline
- approval queue
- transcript viewer
- final diff summary

### 6. Notifications

- local webhook output first
- optional Telegram/Discord later through user-owned adapters
- no embedded bot credentials in core

### 7. Persistence

File-backed run directory:

```text
.ralphterm/runs/<run-id>/
  run.json
  events.jsonl
  phases/
    01-planning/transcript.raw.txt
    02-implementation/transcript.raw.txt
    03-independent-review/transcript.raw.txt
  diff.patch
  summary.md
```

### 8. Safety controls

- idle timeout
- max runtime
- max concurrent sessions
- approval policy
- workspace allowlist
- local API token for non-local bind

## Bite-sized implementation tasks

### Task 1: Rename old planning language

**Objective:** Remove external project naming from every tracked file.

**Files:**
- Modify: `README.md`
- Modify: `Cargo.toml`
- Modify: `docs/plans/2026-05-15-pty-multiplexor.md`

**Verification:**

```bash
git grep -i "external-name-placeholder" || true
```

Expected: no matches for the retired external name.

### Task 2: Add file-backed run storage

**Objective:** Persist run metadata and event logs under `.ralphterm/runs`.

**Files:**
- Create: `src/runs.rs`
- Modify: `src/main.rs`
- Test: unit tests in `src/runs.rs`

**Steps:**

1. Define `RunRecord`, `RunPhase`, `RunStatus`.
2. Add `RunStore::create(base_dir, request)`.
3. Write `run.json` and append `events.jsonl`.
4. Add tests using a temp directory.
5. Commit: `feat: persist run history`.

### Task 3: Add run API

**Objective:** Add workflow-level endpoints above raw sessions.

**Files:**
- Modify: `src/main.rs`
- Modify: `src/runs.rs`

**Endpoints:**

```http
POST /v1/runs
GET  /v1/runs
GET  /v1/runs/:id
GET  /v1/runs/:id/events
POST /v1/runs/:id/cancel
```

**Verification:**

```bash
cargo test --all
```

### Task 4: Implement workspace manager

**Objective:** Create isolated workspaces for each run.

**Files:**
- Create: `src/workspace.rs`
- Test: `src/workspace.rs`

**Behavior:**

- detect git repo
- capture base commit
- create branch/worktree under `.ralphterm/workspaces`
- clean up only on explicit command

### Task 5: Implement phase runner

**Objective:** Convert one run into ordered PTY sessions.

**Files:**
- Create: `src/phases.rs`
- Modify: `src/store.rs`
- Modify: `src/runs.rs`

**Behavior:**

1. planning session
2. implementation session
3. validation command run
4. independent-review session
5. accept and commit only after the review gate passes

### Task 6: Add dashboard shell

**Objective:** Serve a static dashboard from the daemon.

**Files:**
- Create: `dashboard/index.html`
- Create: `dashboard/app.js`
- Create: `dashboard/styles.css`
- Modify: `src/main.rs`

**Verification:**

Open `http://127.0.0.1:7878/dashboard` and verify active sessions render.

### Task 7: Add approval queue

**Objective:** Detect approval prompts and require operator action by default.

**Files:**
- Create: `src/approvals.rs`
- Modify: `src/signals.rs`
- Modify: `src/store.rs`

**Behavior:**

- emit `approval-requested`
- add `POST /v1/sessions/:id/approval`
- log manual or policy-based decisions

### Task 8: Add timeout controls

**Objective:** Prevent runaway sessions.

**Files:**
- Modify: `src/store.rs`
- Modify: `src/pty_agent.rs`

**Controls:**

- idle timeout
- max runtime
- max concurrent sessions

### Task 9: Add final result generation

**Objective:** Produce `summary.md` and `diff.patch` for each run.

**Files:**
- Create: `src/results.rs`
- Modify: `src/runs.rs`

**Verification:**

A successful run directory contains a human-readable summary and patch.

### Task 10: Ship documentation site

**Objective:** Publish polished product docs and a landing page from the repository.

**Files:**
- Modify: `site/index.html`
- Create: `docs/getting-started.md`
- Create: `docs/workflows.md`

**Verification:**

Static page opens locally and links to core docs.

## Milestone demo script

```bash
ralphterm serve --bind 127.0.0.1:7878
ralphterm run \
  --repo ~/work/example \
  --agent claude \
  --task ./task.md \
  --policy manual
```

Expected result:

- dashboard shows run phases
- terminal output streams live
- transcripts are saved
- review phase runs
- final patch and summary are produced
