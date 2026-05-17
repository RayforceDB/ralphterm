# Workflows

RalphTerm starts as a PTY session API and grows into a complete autonomous engineering workflow engine.

## Raw session workflow

Use this when another tool owns planning and orchestration.

1. Create a session.
2. Stream events.
3. Send follow-up input when needed.
4. Watch for signals.
5. Fetch transcript.
6. Cancel or finalize.

## Autonomous engineering workflow

Milestone 1 adds run-level orchestration.

```text
task -> plan -> implement -> self-review -> external-review -> finalize
```

Each phase is backed by one or more terminal sessions. Every phase gets a transcript and structured events.

## Review workflow

Use `--require-review` when a task should not be executed or accepted unless an independent reviewer is configured. You can use a built-in reviewer agent or a custom command.

## Acceptance gates

In a reviewed run, agent completion is only the first gate. RalphTerm accepts progress after this sequence:

1. implementation signal: the implementation PTY emits `COMPLETED`
2. validation pass: every plan-level validation command exits successfully
3. independent review pass: a separate reviewer PTY prints `REVIEW_PASS`
4. plan checkbox + commit: RalphTerm marks the task `[x]` and creates the local checkpoint commit unless `--no-commit` is set

If validation or review fails, the task stays unchecked. If the first reviewer returns `REVIEW_FAIL` and retries are available, RalphTerm sends that feedback back to the implementation agent before trying the gates again.

```bash
ralphterm run docs/plans/example.md --agent claude \
  --require-review \
  --review-agent codex

ralphterm run docs/plans/example.md --agent claude \
  --require-review \
  --review-command "codex exec review-task"
```

`--require-review` is a gate for plan runs. If it is set without `--review-command` or `--review-agent`, RalphTerm fails before starting the implementation agent. That includes `--dry-run`, so a safe preview catches a missing review gate before any real run can accept unreviewed work. The implementation and review commands must also be different; the same command is rejected in dry-run too. With review configured, RalphTerm starts the reviewer in a fresh PTY after validation. The prompt includes:

- task text
- implementation transcript
- validation output
- current git diff and untracked file names

The reviewer must print one exact decision line:

- `REVIEW_PASS` accepts the task
- `REVIEW_FAIL` rejects the current attempt and, by default, triggers one REVIEW_FAIL retry with the reviewer feedback sent back to the implementation agent

If the retry also fails review, RalphTerm leaves the task unchecked and exits failed instead of committing partial progress. Use `--max-review-retries N` to configure the review retry budget: higher values allow more failed reviews before blocking, and `--max-review-retries 0` blocks on the first failed review. Dry-run output and `summary.json` include that retry budget, so scripts can verify the gate before starting real agents. If no reviewer is configured and `--require-review` is not set, RalphTerm prints `Review: skipped`. That mode is useful for smoke tests only.

## Validation, resume, and artifacts

Plan-level validation commands from the `## Validation Commands` section run after implementation and before review. The reviewer prompt includes validation output, the implementation transcript, the current git diff, and untracked file names.

You can resume after a failed run by invoking the same plan again. RalphTerm skips tasks already marked `[x]`, keeps failed context available to the next implementation prompt, and continues with pending tasks.

Plan runs preserve transcripts for implementation and review attempts. They also write progress logs, a summary, and diff artifacts. After validation and `REVIEW_PASS`, RalphTerm marks the task checkbox and commits task progress unless `--no-commit` is set. Failed validation or review leaves the task uncommitted for follow-up.

## Workspace-isolated plan runs

Add `--workspace-id <id>` when a plan run should happen in a managed git worktree instead of the checkout you invoked from.

```bash
ralphterm run docs/plans/example.md --workspace-id docs-slice --agent claude
```

RalphTerm creates `.ralphterm/workspaces/<id>`, checks out a managed branch for the workspace, preserves the caller-relative plan path, and runs from the corresponding directory inside the worktree. The run does not auto-clean the worktree when it finishes, so you can inspect the isolated branch and files before removing them with `ralphterm workspace cleanup <id>`. With `--dry-run --workspace-id <id>`, dry run only previews the workspace path and pending plan work; it does not create the worktree or start an agent.

## Approval workflow

Default mode is manual.

1. Agent output appears to request approval.
2. RalphTerm emits `approval-requested`.
3. Dashboard shows the request.
4. Operator approves, denies, or types a custom response.
5. Decision is logged.

## Result workflow

A completed run should produce:

- `summary.md`
- `summary.json`
- `diff.patch`
- phase transcripts
- `events.jsonl`
- final status

## Future workflow adapters

RalphTerm should integrate with any system that can call a command or HTTP API. The generic adapter reads a prompt from stdin, creates a RalphTerm session, streams output to stdout, and exits according to the final session status.

## Review-only and external-only modes

Ralphex exposes three operational modes besides the default full run, and RalphTerm honors all three.

- `--tasks-only` (`-t`) runs only the implementation phase. Use it for quick iteration when the review gate would slow you down, or for plans that have no separate reviewer.
- `--review` (`-r`) skips task execution and runs the reviewer pipeline once against the current working tree. This is the right mode when you want to validate an already-implemented plan against an independent reviewer.
- `--external-only` (`-e`, alias `--codex-only` / `-c`) runs the external review/fixer loop without implementation. This is the ralphex pattern for handing a plan to a stricter reviewer in a second pass.

All three modes accept the same configuration knobs. `--review` and `--external-only`/`--codex-only` require `--external-review-tool=custom --custom-review-script <cmd>` so RalphTerm has a reviewer to run.

```bash
ralphterm --review \
  --external-review-tool=custom \
  --custom-review-script "codex exec review-task" \
  docs/plans/example.md
```

## Docker-isolated runs

Add `--docker` to wrap the implementation and reviewer commands in a `docker run` invocation. RalphTerm passes the wrapped command to the same PTY runner, so the in-container CLI gets a real TTY.

The default image is `ralphterm:latest` (build it from `docker/Dockerfile`); override with `--docker-image`. Use `--preserve-anthropic-api-key` to forward the host's `ANTHROPIC_API_KEY` env var into the container. The wrapper also honors `RALPHEX_EXTRA_VOLUMES`, `RALPHEX_EXTRA_ENV`, `TZ`, `AWS_PROFILE`, and `AWS_REGION` to match ralphex's passthrough semantics.

See [docker.md](docker.md) for the full reference.

```bash
ralphterm --docker --docker-image ralphterm:latest \
  --preserve-anthropic-api-key docs/plans/example.md
```

## Notifications

RalphTerm can deliver plan-run events to Telegram, Slack, generic HTTP webhooks, and SMTP email. Each channel runs on its own background thread with a 10-second per-delivery timeout, so notification slowness or failure never blocks the run.

Enable a channel with the matching CLI flag (`--notify-slack`, `--notify-webhook`, `--notify-telegram-token` + `--notify-telegram-chat`, or `--notify-email-smtp-url` + `--notify-email-from` + `--notify-email-to`). Filter the events with `--notify-on plan_done,task_failed,review_failed,rate_limit`. Configuration can also live in `~/.config/ralphex/config` or `.ralphex/config.json`.

The core notifier is non-TLS — HTTPS Slack/webhook URLs and `smtps://` SMTP URLs are skipped with a warning. Front the integration with a local TLS proxy or use plain endpoints. See [notifications.md](notifications.md) for the full reference and event schemas.

```bash
ralphterm --tasks-only \
  --notify-slack https://hooks.slack.example/T/B/X \
  --notify-on plan_done,task_failed docs/plans/example.md
```

## Provider wrappers

RalphTerm drives Codex, GitHub Copilot, Google Gemini, and OpenCode through small POSIX wrapper scripts in `scripts/wrappers/`. Each wrapper translates RalphTerm's PTY-driven loop into the upstream CLI's interactive mode and emits the `COMPLETED` / `FAILED` markers the orchestrator expects.

Pick a wrapper by pointing `--claude-command` at it, or set `[agent].provider = codex|copilot|gemini|opencode` in `~/.config/ralphex/config` and RalphTerm resolves the bundled wrapper automatically. An explicit `claude_command` always wins.

See [providers.md](providers.md) for the wrapper contract and per-provider env vars.

```bash
ralphterm --tasks-only \
  --claude-command "$(pwd)/scripts/wrappers/codex.sh" \
  docs/plans/example.md
```

## Plan completion

Pass `--move-completed` (or set `move_plan_on_completion = true` in config) to move successfully completed plans into `<plan-dir>/completed/`. Only `--tasks-only` and full-mode runs are eligible to move — review-only and external-only modes never move plans because they did not produce acceptance.

The move runs after the local checkpoint commit and after the ralphex progress symlink is refreshed, so dashboards that tail `.ralphex/progress/` continue to see the run summary even after the plan moves. RalphTerm prints the canonical destination path so scripts can update follow-up references.

```bash
ralphterm --tasks-only --move-completed docs/plans/example.md
# => Moved plan to /…/docs/plans/completed/example.md
```
