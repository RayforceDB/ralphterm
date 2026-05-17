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

Use the run API when RalphTerm owns the plan loop, not just the terminal session. The daemon creates a run record under `.ralphterm/runs/<id>/`, starts execution in the background, stores events, and writes result artifacts when the run finishes.

```http
POST /v1/runs
content-type: application/json
```

Request:

```json
{
  "plan_path": "docs/plans/example.md",
  "agent": "claude",
  "review_agent": "codex",
  "require_review": true,
  "max_review_retries": 1,
  "no_commit": false
}
```

Fields:

- `plan_path`: markdown plan path, relative to the daemon working directory or absolute
- `agent`: optional built-in command shortcut resolved from `PATH`, either `claude` or `codex`; conflicts with `agent_command`
- `agent_command`: optional raw implementation command; omit both `agent` and `agent_command` to create a run record without starting work
- `review_agent`: optional built-in reviewer command shortcut resolved from `PATH`, either `claude` or `codex`; conflicts with `review_command`
- `review_command`: optional raw independent reviewer command
- `require_review`: rejects the request unless `review_agent` or `review_command` is set
- `max_review_retries`: number of review failures allowed before the task blocks
- `no_commit`: marks accepted tasks and writes artifacts without creating git commits

Response when execution starts:

```json
{
  "id": "00000000-0000-0000-0000-000000000000",
  "created_at": "unix-ms:1778954400000",
  "phase": "executing",
  "status": "running",
  "plan_path": "docs/plans/example.md"
}
```

`POST /v1/runs` returns as soon as the run has started. Poll `GET /v1/runs/:id` for `succeeded` or `failed`, or read `GET /v1/runs/:id/events` for lifecycle events. If both `agent` and `agent_command` are omitted, the daemon only creates the run record and returns `phase: "planning"`, `status: "created"`.

Run phase values are `planning`, `executing`, `reviewing`, and `complete`: `planning` means the run record exists without active agent work, `executing` means the implementation command or agent is active, reviewing means the independent review command or agent is active, and `complete` means the run has reached a terminal status.

Run endpoints:

`GET /v1/runs` returns newest runs first so dashboards and watchdogs see the active work before older records.

```http
GET  /v1/runs
GET  /v1/runs/:id
GET  /v1/runs/:id/summary
GET  /v1/runs/:id/summary.json
GET  /v1/runs/:id/diff
GET  /v1/runs/:id/progress
GET  /v1/runs/:id/progress/:artifact
GET  /v1/runs/:id/events
POST /v1/runs/:id/cancel
```

`GET /v1/runs/:id/events` returns run lifecycle events such as `run_created`, `run_succeeded`, `run_failed`, and `run_cancelled`. Plan-runner executions write `summary.md` and `diff.patch` under `.ralphterm/runs/<id>/`; agent-backed runs also preserve the runner-generated `summary.json` when the runner produced one. `GET /v1/runs/:id/summary` returns `summary.md` as plain text; `GET /v1/runs/:id/summary.json` returns the machine-readable task result summary when present. Each task includes `accepted` and `acceptance_gates` so callers can see the agent, validation, review, and commit gates without inferring acceptance from transcript paths. `GET /v1/runs/:id/diff` returns `diff.patch` as plain text. `GET /v1/runs/:id/progress` returns a JSON index of copied progress artifacts with `name`, `kind`, and `url` fields for transcripts, validation output, review transcripts, and the progress log. Follow an artifact `url`, or call `GET /v1/runs/:id/progress/:artifact`, to fetch the copied transcript, validation output, review transcript, or progress log directly. Runs that exist but do not have an artifact yet return 404 with an artifact-specific message.

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
  "approval_pending": false,
  "exit_code": null,
  "created_at_ms": 0,
  "updated_at_ms": 0
}
```

`approval_pending` is `true` while the session has pending approval and is waiting for an explicit approval response; otherwise it is `false`.

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

## List sessions

```http
GET /v1/sessions
```

Returns the in-memory session records for the running daemon. The dashboard uses this endpoint to render active and recently exited PTY sessions.

```json
[
  {
    "id": "00000000-0000-0000-0000-000000000000",
    "agent": "claude",
    "status": "running",
    "signal": null,
    "approval_pending": false,
    "exit_code": null
  }
]
```

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
