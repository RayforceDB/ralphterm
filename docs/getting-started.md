# Getting Started

RalphTerm is a drop-in replacement for [ralphex](https://github.com/umputun/ralphex). The fastest way to learn it is to point it at an existing plan and run the same command you would run with ralphex.

## Install

From source:

```bash
git clone git@github.com:RayforceDB/ralphterm.git
cd ralphterm
cargo install --path .
```

This installs both `ralphterm` and the bundled `ralphex` alias. Confirm:

```bash
ralphterm --version
ralphex --version
```

If you only want to run from the checkout without installing, `cargo build --release` produces `./target/release/ralphterm` and `./target/release/ralphex`.

## Point at a plan

Plans are markdown files with unchecked task items and a `## Validation Commands` block. A minimal example:

```markdown
# Example plan

## Validation Commands
- `cargo test --all`

### Task 1: Add the smallest useful slice
- [ ] Write the failing test first
- [ ] Implement the slice
- [ ] Run the validation command
```

Save it as `docs/plans/example.md` (or anywhere you like).

## Run it (ralphex-style)

The drop-in command form is the same as ralphex:

```bash
ralphex --tasks-only docs/plans/example.md
```

That runs only the implementation phase: it skips the review gate, sends each pending task to the agent inside a real PTY, runs the validation commands, marks the task `[x]` on success, and commits a local checkpoint (unless you pass `--no-commit`).

`ralphterm` is the same binary under a different name:

```bash
ralphterm --tasks-only docs/plans/example.md
```

## Add the review gate

Default (full) mode requires an independent reviewer. Configure one with `--external-review-tool=custom --custom-review-script <cmd>`:

```bash
ralphterm \
  --external-review-tool=custom \
  --custom-review-script "codex exec review-task" \
  docs/plans/example.md
```

After implementation succeeds and validation passes, the reviewer sees the transcript, validation output, and git diff in a fresh PTY along with the task text. It must print `REVIEW_PASS` for RalphTerm to mark the task `[x]` and commit. A `REVIEW_FAIL` triggers a single implementation retry with the reviewer's feedback; a second failure leaves the task unchecked.

The same gate can be persisted in `~/.config/ralphex/config`:

```ini
external_review_tool = custom
custom_review_script = codex exec review-task
```

## Native subcommand path

For users who want the explicit RalphTerm interface, the `run` subcommand exposes the same behavior with named flags:

```bash
ralphterm run docs/plans/example.md --dry-run
ralphterm run docs/plans/example.md --agent claude \
  --require-review \
  --review-command "codex exec review-task"
```

`--dry-run` prints pending tasks, review mode, retry budget, and validation commands without starting an agent or editing the plan.

To isolate the run in a managed git worktree, add `--workspace-id <id>`:

```bash
ralphterm run docs/plans/example.md --workspace-id docs-slice --agent claude
```

RalphTerm creates `.ralphterm/workspaces/<id>`, resolves the caller-relative plan path inside the worktree, and runs from there. It does not auto-clean the worktree; remove it later with `ralphterm workspace cleanup <id>`.

## Start the daemon

For the REST + WebSocket API:

```bash
ralphterm serve --bind 127.0.0.1:7878
curl http://127.0.0.1:7878/health
```

Expected:

```json
{"ok":true}
```

The ralphex-compatible `--serve` flag also works:

```bash
ralphex --serve --port 7878 --host 127.0.0.1
```

See [`docs/api.md`](api.md) for the API contract.

## Where to go next

- Compatibility matrix: [`docs/ralphex-compat.md`](ralphex-compat.md)
- Full CLI reference: [`docs/cli-reference.md`](cli-reference.md)
- Migration guide: [`docs/migrate-from-ralphex.md`](migrate-from-ralphex.md)
- Notifications: [`docs/notifications.md`](notifications.md)
- Docker isolation: [`docs/docker.md`](docker.md)
- Alternate providers: [`docs/providers.md`](providers.md)
- Workflows: [`docs/workflows.md`](workflows.md)

## Development checks

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```
