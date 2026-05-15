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

A reviewer agent receives:

- task text
- implementation summary
- git diff
- relevant transcript excerpts

The reviewer must produce either:

- `REVIEW_DONE` with approval
- `FAILED` with blockers
- `QUESTION` for human input

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
