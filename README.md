# RalphTerm

[![CI](https://github.com/RayforceDB/ralphterm/actions/workflows/ci.yml/badge.svg)](https://github.com/RayforceDB/ralphterm/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-00d992.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.95%2B-f2f2f2?logo=rust)](Cargo.toml)
[![Website](https://img.shields.io/badge/website-ralphterm.rayforcedb.com-00d992)](https://ralphterm.rayforcedb.com)
[![Social Preview](https://img.shields.io/badge/social-preview-818cf8)](https://ralphterm.rayforcedb.com/assets/social-preview.png)

**A safer replacement for brittle one-shot AI CLI automation.**

RalphTerm solves a concrete problem: many autonomous coding stacks depend on one-shot CLI prompt flags. That path gets fragile when the CLI asks for approval, changes output format, needs follow-up input, hits auth, or moves more behavior into the interactive terminal. RalphTerm runs Claude Code, Codex, and future terminal agents inside real PTYs, then gives orchestration systems a clean API around that session.

The point is not to bypass the vendors. The point is to stop pretending the terminal is not there. The official CLI still owns login, rate limits, safety prompts, and account identity. RalphTerm owns session control, streaming, transcripts, signals, and approvals.

## Links

- Website: [ralphterm.rayforcedb.com](https://ralphterm.rayforcedb.com)
- Documentation: [ralphterm.rayforcedb.com/docs/](https://ralphterm.rayforcedb.com/docs/)
- Workflows: [ralphterm.rayforcedb.com/docs/workflows.html](https://ralphterm.rayforcedb.com/docs/workflows.html)
- Social preview: [assets/social-preview.png](https://ralphterm.rayforcedb.com/assets/social-preview.png)
- Milestone 1: [`docs/milestones/m1-autonomous-engineering.md`](docs/milestones/m1-autonomous-engineering.md)
- Security model: [`docs/security.md`](docs/security.md)

## Why RalphTerm exists

AI coding tools are becoming interactive terminal products. Automation built around non-interactive prompt mode is fragile. RalphTerm takes the durable path: run the real CLI in a real terminal and build a reliable control plane around it.

## What RalphTerm does today

- Replaces one-shot prompt-mode execution with one isolated PTY session per agent run.
- Supports Claude Code and Codex as first-class agents.
- Sends prompts and follow-up input as terminal keystrokes.
- Streams raw terminal output over WebSocket.
- Keeps transcripts and status for every session.
- Detects workflow signals such as `COMPLETED`, `FAILED`, `PLAN_READY`, `QUESTION`, and `REVIEW_DONE`.
- Exposes REST controls for create, input, resize, cancel, status, transcript, and events.
- Binds to `127.0.0.1` by default because the API controls local terminals.

## Quick start

```bash
git clone git@github.com:RayforceDB/ralphterm.git
cd ralphterm
cargo run -- serve --bind 127.0.0.1:7878
```

Health check:

```bash
curl http://127.0.0.1:7878/health
```

Expected:

```json
{"ok":true}
```

## Current API

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

Deterministic smoke test using `/bin/sh` as the command override:

```bash
ID=$(curl -sS -X POST http://127.0.0.1:7878/v1/sessions \
  -H 'content-type: application/json' \
  -d '{
    "agent":"claude",
    "command":"/bin/sh",
    "args":["-lc","read line; printf \"%s\\n\" \"$line\"; echo COMPLETED"],
    "prompt":"hello from ralphterm"
  }' | python3 -c 'import sys,json; print(json.load(sys.stdin)["id"])')

curl http://127.0.0.1:7878/v1/sessions/$ID
curl http://127.0.0.1:7878/v1/sessions/$ID/transcript
```

Manual real CLI smoke test and plan run:

```bash
ralphterm smoke --agent claude
ralphterm run docs/plans/example.md --dry-run
ralphterm run docs/plans/example.md --agent claude \
  --require-review \
  --review-command "codex exec review-task"
ralphterm run docs/plans/example.md --workspace-id docs-slice --agent claude
```

`--require-review` makes review mandatory for a plan run. If it is set without `--review-command` or `--review-agent`, RalphTerm fails before starting the implementation agent, so it cannot accept or execute tasks without an independent review configuration. Use `--review-agent codex` for a built-in reviewer CLI, or `--review-command <cmd>` for a custom reviewer command. The reviewer sees the task text, implementation transcript, validation output, and current git state. It must print `REVIEW_PASS` before RalphTerm marks the task `[x]` or commits. By default, the first `REVIEW_FAIL` triggers one retry with reviewer feedback sent back to the implementation agent; a second review failure leaves the task unchecked and prevents the commit. Use `--max-review-retries N` to allow more review-driven retries, or `--max-review-retries 0` to block on the first failed review.

Start with `ralphterm smoke --agent claude` or `ralphterm smoke --agent codex` to verify the official CLI can start inside a real PTY, receive terminal input, print `COMPLETED`, and exit. Then use `--dry-run` to see the pending tasks, review mode, and validation commands without starting an agent, editing the plan, writing progress logs, or committing. Run the real plan command only after the official Claude Code CLI is installed, authenticated, and works interactively as `claude` in your shell. RalphTerm launches the interactive CLI in a PTY and sends terminal input; it does not use `claude -p`, `--print`, or any one-shot prompt mode. Use `--agent codex` to run the same workflow with an authenticated interactive `codex` CLI. The lower-level `--agent-command <cmd>` option remains available for tests and custom command wrappers.

Use `ralphterm run PLAN --workspace-id <id>` when the plan should run in a managed git worktree instead of the checkout you invoked from. RalphTerm creates `.ralphterm/workspaces/<id>`, switches into the matching caller-relative plan path inside that worktree, and runs the plan from there. The run does not auto-clean the worktree; inspect it or remove it later with `ralphterm workspace cleanup <id>`. With `--dry-run --workspace-id <id>`, dry run only previews the workspace path and plan work without creating the worktree or running an agent.

## Milestone 1

Milestone 1 is to ship a complete autonomous engineering workflow on top of RalphTerm's PTY core:

- task intake and planning
- isolated workspaces
- multi-agent execution
- review loops
- approval queue
- status dashboard
- notifications
- transcript and event audit trail
- final patch and summary artifacts
- local-first API and CLI

See [`docs/milestones/m1-autonomous-engineering.md`](docs/milestones/m1-autonomous-engineering.md).

## Documentation

- [`docs/product.md`](docs/product.md) — product positioning and principles
- [`docs/api.md`](docs/api.md) — current API contract
- [`docs/architecture.md`](docs/architecture.md) — daemon, PTY runtime, events, storage
- [`docs/security.md`](docs/security.md) — compliance and safety model
- [`docs/getting-started.md`](docs/getting-started.md) — local development quickstart
- [`docs/workflows.md`](docs/workflows.md) — run and review workflows
- [`site/`](site/) — static landing website and hosted docs

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```

## Compliance stance

RalphTerm is a terminal multiplexer and orchestration layer, not a protocol bypass. It launches official user-installed CLIs in PTYs, leaves auth and rate limits to those CLIs, and requires explicit user-configured approval policy for automation. It does not emulate private APIs, store provider credentials, alter account identity, or bypass interactive safety prompts by default.

## License

MIT. See [`LICENSE`](LICENSE).
