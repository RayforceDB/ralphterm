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

Use `--require-review` with `--review-command <cmd>` when a task should not be executed or accepted unless an independent reviewer is configured.

```bash
ralphterm run docs/plans/example.md --agent claude \
  --require-review \
  --review-command "codex exec review-task"
```

`--require-review` is a gate for plan runs. If it is set without `--review-command`, RalphTerm fails before starting the implementation agent. With both flags set, RalphTerm starts the reviewer in a fresh PTY after validation. The prompt includes:

- task text
- implementation transcript
- validation output
- current git diff and untracked file names

The reviewer must print one exact decision line:

- `REVIEW_PASS` accepts the task
- `REVIEW_FAIL` blocks `[x]`, commit, and run summary

If no review command is configured and `--require-review` is not set, RalphTerm prints `Review: skipped`. That mode is useful for smoke tests only.

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
- `diff.patch`
- phase transcripts
- `events.jsonl`
- final status

## Future workflow adapters

RalphTerm should integrate with any system that can call a command or HTTP API. The generic adapter reads a prompt from stdin, creates a RalphTerm session, streams output to stdout, and exits according to the final session status.
