# ralphterm

PTY-backed agent multiplexor for ralphex-style workflows.

Goal: keep the automation surface compatible with tools that expect one-shot agent runs, but drive Claude Code and Codex exactly like a user would in an interactive terminal. No `claude -p`, no hidden provider API calls, no scraping private APIs. Users authenticate with the official CLIs and the mux only types into their local PTYs, reads terminal output, and exposes session control over a local API.

## MVP

- Spawn Claude/Codex in a dedicated PTY per session.
- Send prompts as pasted user input.
- Stream terminal output via WebSocket/SSE.
- Detect ralphex signals like `COMPLETED`, `FAILED`, `PLAN_READY`, `QUESTION`, `REVIEW_DONE`.
- Expose REST API for start, input, resize, cancel, status, and transcript.
- Support safe approval policy hooks without bypassing product terms.

## Local API sketch

```bash
ralphterm serve --bind 127.0.0.1:7878
```

```http
POST /v1/sessions
GET  /v1/sessions/:id
POST /v1/sessions/:id/input
POST /v1/sessions/:id/resize
POST /v1/sessions/:id/cancel
GET  /v1/sessions/:id/events
GET  /v1/sessions/:id/transcript
```

## Compliance stance

This tool is a terminal multiplexer, not a protocol bypass. It launches official user-installed CLIs in PTYs, leaves auth and rate limits to those CLIs, and requires user-configured approval policy. It does not emulate private APIs, share credentials, alter account identity, or bypass interactive safety prompts by default.
