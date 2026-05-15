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

## Create session

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
