# RalphTerm

**Terminal-native orchestration for AI coding agents.**

RalphTerm turns user-installed AI CLIs into durable, observable, API-controlled workers. It launches Claude Code, Codex, and future terminal agents inside real PTYs, types like a user, streams what the terminal prints, and exposes clean controls for orchestration systems.

No brittle one-shot prompt modes. No private provider APIs. No credential copying. The official CLI keeps owning login, rate limits, safety prompts, and account identity.

## Why this exists

Modern AI coding tools are becoming interactive terminal products. Automation built on hidden flags or non-interactive prompt modes is fragile. RalphTerm takes the boring, durable path: run the real CLI in a real terminal and build a reliable control plane around it.

## What RalphTerm does

- Starts one isolated PTY session per agent run.
- Supports Claude Code and Codex as first-class agents.
- Sends prompts and follow-up input as terminal keystrokes.
- Streams raw terminal output over WebSocket.
- Keeps transcripts and status for every session.
- Detects workflow signals such as `COMPLETED`, `FAILED`, `PLAN_READY`, `QUESTION`, and `REVIEW_DONE`.
- Exposes REST controls for create, input, resize, cancel, status, transcript, and events.
- Binds to `127.0.0.1` by default because the API controls local terminals.

## Current API

```bash
ralphterm serve --bind 127.0.0.1:7878
```

```http
GET  /health
POST /v1/sessions
GET  /v1/sessions/:id
POST /v1/sessions/:id/input
POST /v1/sessions/:id/resize
POST /v1/sessions/:id/cancel
GET  /v1/sessions/:id/events
GET  /v1/sessions/:id/transcript
```

Example deterministic test session:

```bash
curl -sS -X POST http://127.0.0.1:7878/v1/sessions \
  -H 'content-type: application/json' \
  -d '{
    "agent":"claude",
    "command":"/bin/sh",
    "args":["-lc","read line; printf \"%s\\n\" \"$line\"; echo COMPLETED"],
    "prompt":"hello from ralphterm"
  }'
```

## Milestone 1

Milestone 1 is to ship a complete autonomous engineering workflow on top of RalphTerm's PTY core:

- task intake and planning
- isolated workspaces
- multi-agent execution
- review loops
- status dashboard
- notifications
- transcript/audit trail
- local-first API and CLI

See [`docs/milestones/m1-autonomous-engineering.md`](docs/milestones/m1-autonomous-engineering.md).

## Documentation

- [`docs/product.md`](docs/product.md) — product positioning and principles
- [`docs/api.md`](docs/api.md) — current API contract
- [`docs/architecture.md`](docs/architecture.md) — daemon, PTY runtime, events, storage
- [`docs/security.md`](docs/security.md) — compliance and safety model
- [`site/index.html`](site/index.html) — marketing landing page draft

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```

## Compliance stance

RalphTerm is a terminal multiplexer, not a protocol bypass. It launches official user-installed CLIs in PTYs, leaves auth and rate limits to those CLIs, and requires explicit user-configured approval policy for automation. It does not emulate private APIs, store provider credentials, alter account identity, or bypass interactive safety prompts by default.
