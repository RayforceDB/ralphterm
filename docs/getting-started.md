# Getting Started

## Install from source

```bash
git clone git@github.com:RayforceDB/ralphterm.git
cd ralphterm
cargo build --release
```

## Start the daemon

```bash
cargo run -- serve --bind 127.0.0.1:7878
```

## Check health

```bash
curl http://127.0.0.1:7878/health
```

Expected:

```json
{"ok":true}
```

## Run a deterministic test session

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

## Run with real CLIs

Install and authenticate the official tools first:

```bash
claude auth login
codex login
```

Then create a session without `command` override:

```bash
curl -sS -X POST http://127.0.0.1:7878/v1/sessions \
  -H 'content-type: application/json' \
  -d '{"agent":"claude","prompt":"Say hello and end with COMPLETED"}'
```

## Manual `ralphterm run` smoke test

Preview the plan first:

```bash
ralphterm run docs/plans/example.md --dry-run
```

That prints pending tasks, review mode, and validation commands only. It does not start an agent, edit the plan, write `.ralphterm/progress/`, or commit.

To isolate a real plan run from your current checkout, add `--workspace-id <id>`:

```bash
ralphterm run docs/plans/example.md --workspace-id docs-slice --agent claude
```

RalphTerm creates a managed git worktree at `.ralphterm/workspaces/<id>`, resolves the caller-relative plan path before switching directories, and runs from the corresponding path inside that worktree. It does not auto-clean the worktree after the run; inspect it or remove it later with `ralphterm workspace cleanup <id>`. With `--dry-run --workspace-id <id>`, dry run only previews the workspace path and plan work without creating the worktree or running an agent.

A minimal plan is just a validation block plus unchecked task items:

```markdown
# Example plan

## Validation Commands
- `cargo test --all`

### Task 1: Add the smallest useful slice
- [ ] Write the failing test first
- [ ] Implement the slice
- [ ] Run the validation command
```

RalphTerm sends the pending task to the implementation agent, then runs the validation commands. If review is required, the reviewer sees the transcript, validation output, and git diff before it can print `REVIEW_PASS`.

After the official Claude Code CLI is installed, authenticated, and works interactively as `claude` in your shell, run:

```bash
ralphterm run docs/plans/example.md --agent claude \
  --require-review \
  --review-command "codex exec review-task"
```

Use `--require-review` for real plan runs that must have an independent reviewer. When this gate is set, RalphTerm exits before starting the implementation agent unless `--review-command` or `--review-agent` is supplied. Use `--review-agent codex` for a built-in reviewer CLI, or `--review-command <cmd>` for a custom reviewer command. The reviewer runs in a fresh PTY after validation, receives the task, agent transcript, validation output, and git state, and must print an exact `REVIEW_PASS` line. If the reviewer prints `REVIEW_FAIL`, RalphTerm gives that review feedback to one fresh implementation retry by default, re-runs validation, and re-runs review. A second review failure leaves the task unchecked and prevents the commit. Use `--max-review-retries N` to allow more review-driven retries, or `--max-review-retries 0` to block on the first failed review.

RalphTerm launches the interactive CLI in a PTY and sends the task prompt as terminal input. It does not use `claude -p`, `--print`, or any one-shot prompt mode. Use `--agent codex` for an authenticated interactive Codex CLI, or `--agent-command <cmd>` for tests and custom wrappers.

## Development checks

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```
