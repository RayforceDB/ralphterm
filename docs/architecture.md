# RalphTerm Architecture

RalphTerm is a local daemon that turns interactive terminal agents into API-controlled sessions.

## Components

### API server

Rust + axum exposes REST and WebSocket endpoints. The server is local-first and binds to `127.0.0.1` by default.

Responsibilities:

- validate session requests
- expose status and transcripts
- forward input and resize requests
- stream session events
- enforce future auth and limits

### Session store

The store keeps in-memory session records for the MVP:

- session id
- agent kind
- status
- detected signal
- exit code
- created/updated timestamps
- transcript buffer
- event broadcaster
- child handle and control channels

Planned persistence adds transcript files and event logs without changing the public API.

### PTY runtime

Each session gets a dedicated pseudo-terminal.

Flow:

1. create PTY pair
2. spawn the official CLI inside the PTY
3. drop the parent copy of the slave handle
4. write the initial prompt into the terminal
5. read terminal output continuously
6. append transcript and broadcast output events
7. detect workflow signals from recent output
8. wait for child exit and record exit code

### Plan-loop agent driver

`src/agent_driver.rs` is the v0.3 async driver that runs one implementer iteration per call. It is what `run_plan_default` invokes; the lower-level PTY runtime above is its substrate.

Per iteration:

1. generate a 32-hex-character nonce and an output path at `.ralphex/iteration-output/<nonce>.md`
2. write the task prompt (with a protocol preamble pointing at the output path) to `.ralphex/iteration-output/<nonce>.prompt.txt`
3. spawn the CLI promptlessly via portable-pty, exporting `RALPHTERM_OUTPUT_FILE`, `RALPHTERM_PROMPT_FILE`, and `RALPHTERM_NONCE`
4. bridge the blocking PTY reader into a `tokio::sync::mpsc` channel via `spawn_blocking`
5. wait for the Claude alt-screen-buffer sequence (REPL-ready) or auto-dismiss the bypass-permissions dialog with `↓ Enter`
6. paste the prompt via bracketed-paste (`ESC[200~ … ESC[201~`) then submit with `\r`
7. `tokio::select!` over (byte channel, 200 ms file-poll tick, idle-timeout sleep, cancellation watch)
8. when the output file contains the literal `<<<END>>>` line, capture the slice between `<<<BEGIN>>>` and `<<<END>>>` as the response, send `/exit\r` to the PTY, and reap with a 3 s budget

Failure paths: `timed_out`, `cancelled`, `crashed_before_done`, or `agent_exited_without_file`. The runner upgrades `timed_out` to a hard abort; the others are warnings that let the iteration loop reach its `max_iterations` cap. Per-iteration events (`agent_started`, `agent_prompt_pasted`, `agent_prompt_submitted`, `agent_output_file_complete`, etc.) flow into the runner's `RunEventSink` and out through `/v1/runs/:id/events`.

The file-handoff channel is on disk, not in the PTY stream, so it survives Claude TUI rendering quirks (cursor-forward escapes, alt-screen redraws, mid-output redraws) that broke earlier in-band marker designs.

### Agent adapters

An agent adapter maps a logical agent to a real command.

Current defaults:

- `claude` -> `claude`
- `codex` -> `codex`

The MVP deliberately passes no one-shot prompt flags. The prompt enters through the PTY as user input.

### Signal detector

The detector watches terminal text for simple markers:

- completion
- failure
- question/request for human input
- plan ready
- review done

This keeps orchestration independent from any one provider or CLI.

### Notifier

`src/notify.rs` provides a fire-and-forget notification fanout that supports Telegram, Slack, generic HTTP webhooks, and SMTP email. Each delivery runs on its own background thread with a 10-second timeout, so notification slowness or failure never blocks the run. The notifier is intentionally non-TLS (HTTPS endpoints are skipped with a warning) to avoid pulling a heavy HTTP/TLS crate into the core. See [`docs/notifications.md`](notifications.md).

### Docker wrapper

`src/docker.rs` translates an implementer or reviewer command into a `docker run` invocation. The wrapped command is handed back to the PTY runner unchanged, so the in-container CLI gets the same TTY-driven loop as the host path. The wrapper honors ralphex passthrough env vars (`RALPHEX_EXTRA_VOLUMES`, `RALPHEX_EXTRA_ENV`, `TZ`, `AWS_PROFILE`, `AWS_REGION`) and gates `ANTHROPIC_API_KEY` behind `--preserve-anthropic-api-key`. See [`docs/docker.md`](docker.md).

### Provider wrappers

POSIX scripts under `scripts/wrappers/` (and `<exe_dir>/../share/ralphterm/wrappers/` after installation) translate RalphTerm's PTY-driven loop into Codex, Copilot, Gemini, and OpenCode interactive sessions. Each wrapper accepts a single stdin prompt, runs the upstream CLI without `--print`/`--non-interactive` flags, forwards `CLAUDE_MODEL` to the upstream `--model` selector, and emits `COMPLETED` or `FAILED rc=<code>` on exit. `src/config.rs` auto-resolves a wrapper when the global config sets `[agent].provider = codex|copilot|gemini|opencode` and no `claude_command` is configured. See [`docs/providers.md`](providers.md).

### Approval policy engine

Planned for Milestone 1.

Default mode is manual. When terminal output appears to request approval, RalphTerm emits an event. Optional policies can respond only to explicitly configured, low-risk prompts.

### Dashboard

Planned for Milestone 1.

The dashboard reads the same API as external clients. It should show:

- active sessions
- terminal stream
- approval requests
- signals
- transcripts
- run history

## Data model direction

The MVP is memory-only. Milestone 1 should introduce:

```text
.ralphterm/
  runs/
    <run-id>/
      request.json
      events.jsonl
      transcript.raw.txt
      transcript.clean.txt
      summary.md
```

## Failure model

RalphTerm should make failures visible instead of hiding them:

- command not found -> failed session with actionable error
- CLI not logged in -> failed session with transcript
- idle timeout -> timed out session
- user cancellation -> cancelled session
- approval timeout -> waiting/manual action required

## Security boundaries

RalphTerm controls terminals. Treat the API as sensitive. Localhost is safe for development. Remote exposure must require explicit auth and transport hardening.
