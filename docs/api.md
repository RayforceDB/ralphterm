# RalphTerm API

RalphTerm exposes a local HTTP/WebSocket API around terminal agent sessions.

Default bind:

```bash
ralphterm serve --bind 127.0.0.1:7878
```

## Health

```http
GET /health
```

Response:

```json
{"ok": true}
```

## Run a reviewed plan

Use the run API when RalphTerm owns the plan loop, not just the terminal session. The daemon creates a run record under `.ralphterm/runs/<id>/`, executes the markdown plan, stores events, and writes result artifacts.

```http
POST /v1/runs
content-type: application/json
```

Request:

```json
{
  "plan_path": "docs/plans/example.md",
  "agent_command": "claude",
  "review_command": "codex exec review-task",
  "require_review": true,
  "max_review_retries": 1,
  "no_commit": false
}
```

Fields:

- `plan_path`: markdown plan path, relative to the daemon working directory or absolute
- `agent_command`: optional implementation command; omit it to create a run record without starting work
- `review_command`: optional independent reviewer command
- `require_review`: rejects the request unless `review_command` is set
- `max_review_retries`: number of review failures allowed before the task blocks
- `no_commit`: marks accepted tasks and writes artifacts without creating git commits

Response:

```json
{
  "id": "00000000-0000-0000-0000-000000000000",
  "created_at": "unix-ms:1778954400000",
  "phase": "complete",
  "status": "succeeded",
  "plan_path": "docs/plans/example.md"
}
```

Run endpoints:

```http
GET  /v1/runs
GET  /v1/runs/:id
GET  /v1/runs/:id/summary
GET  /v1/runs/:id/diff
GET  /v1/runs/:id/events
POST /v1/runs/:id/cancel
```

`GET /v1/runs/:id/events` returns run lifecycle events such as `run_created`, `run_succeeded`, `run_failed`, and `run_cancelled`. A completed run writes `summary.md` and `diff.patch` under `.ralphterm/runs/<id>/`. `GET /v1/runs/:id/summary` returns `summary.md` as plain text; `GET /v1/runs/:id/diff` returns `diff.patch` as plain text. Runs that exist but do not have an artifact yet return 404 with an artifact-specific message.

## Create a raw session

Use the session API when another orchestrator owns planning and review.

```http
POST /v1/sessions
content-type: application/json
```

Request:

```json
{
  "agent": "claude",
  "prompt": "Review the repository and end with COMPLETED.",
  "cwd": "/path/to/repo",
  "cols": 120,
  "rows": 40
}
```

Fields:

- `agent`: `claude` or `codex`
- `prompt`: text pasted into the PTY after startup
- `cwd`: optional working directory
- `command`: optional test override for deterministic fixtures
- `args`: optional command args
- `cols`, `rows`: PTY size

Response:

```json
{"id":"00000000-0000-0000-0000-000000000000"}
```

## Get status

```http
GET /v1/sessions/:id
```

Response shape:

```json
{
  "id": "uuid",
  "agent": "claude",
  "status": "running",
  "signal": null,
  "exit_code": null,
  "created_at_ms": 0,
  "updated_at_ms": 0
}
```

## Send input

```http
POST /v1/sessions/:id/input
content-type: application/json
```

```json
{
  "text": "Continue with the implementation",
  "enter": true
}
```

Use this for follow-up prompts, explicit approval responses, or operator intervention.

## Resize session

```http
POST /v1/sessions/:id/resize
content-type: application/json
```

```json
{"cols": 160, "rows": 48}
```

## Cancel session

```http
POST /v1/sessions/:id/cancel
```

The daemon sends a termination request to the child process and marks the session as cancelled/exited once the process stops.

## Transcript

```http
GET /v1/sessions/:id/transcript
```

Returns the captured terminal transcript as plain text.

## Events

```http
GET /v1/sessions/:id/events
upgrade: websocket
```

Each WebSocket message is JSON:

```json
{
  "ts_ms": 0,
  "kind": "output",
  "data": "terminal bytes as utf-8 text"
}
```

Planned event kinds:

- `output`
- `signal`
- `approval-requested`
- `approval-sent`
- `error`
- `exit`

## Signals

RalphTerm currently recognizes these terminal markers:

- `COMPLETED`
- `ALL_TASKS_DONE`
- `FAILED`
- `QUESTION`
- `PLAN_READY`
- `REVIEW_DONE`

Signals are intentionally plain text so any terminal agent can emit them.
