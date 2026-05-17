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
