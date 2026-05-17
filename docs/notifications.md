# Notifications

RalphTerm can fan out plan-run events to Telegram, Slack, generic HTTP webhooks, and SMTP email. The notifier is fire-and-forget: every channel gets its own background thread with a 10-second delivery timeout, and delivery errors never block or fail the run.

## What gets delivered

The notifier fires on four event types:

| Event | When | Filter key |
| --- | --- | --- |
| Plan done | Full or `--tasks-only` run finishes successfully. | `plan_done` |
| Task failed | A task fails implementation or validation. | `task_failed` |
| Review failed | A review iteration exhausts the retry budget. | `review_failed` |
| Rate limit | The runtime detects a provider rate-limit pause. | `rate_limit` |

Filter the events with `--notify-on <list>` or the `notify_on` config key. When the filter is empty, every event is delivered.

## Channels

### Telegram

```bash
ralphterm --tasks-only \
  --notify-telegram-token <bot-token> \
  --notify-telegram-chat <chat-id> \
  --notify-on plan_done docs/plans/example.md
```

By default the notifier talks to `https://api.telegram.org`. Because the core notifier is non-TLS, HTTPS endpoints are skipped with a warning. To exercise Telegram locally, override the base URL:

```bash
export RALPHTERM_TELEGRAM_BASE=http://127.0.0.1:8081
```

Tests use this hook to point at an in-process HTTP server.

### Slack

```bash
ralphterm --tasks-only \
  --notify-slack https://hooks.slack.example/T/B/X \
  --notify-on plan_done,task_failed docs/plans/example.md
```

Slack webhooks accept JSON `{"text": "..."}` payloads. The notifier formats each event with a subject line and a short body.

### Generic HTTP webhook

```bash
ralphterm --tasks-only \
  --notify-webhook http://localhost:9000/ralphterm \
  --notify-on plan_done docs/plans/example.md
```

The webhook receives a `POST` with JSON. The body schema depends on the event:

```json
{ "event": "plan_done", "plan": "example.md", "summary": "…" }
{ "event": "task_failed", "plan": "example.md", "task": "1", "reason": "…" }
{ "event": "review_failed", "plan": "example.md", "task": "1", "reason": "…" }
{ "event": "rate_limit_detected", "detail": "…" }
```

### SMTP email

```bash
ralphterm --tasks-only \
  --notify-email-smtp-url smtp://user:pass@mail.example:587 \
  --notify-email-from ralphterm@example \
  --notify-email-to ops@example \
  --notify-on plan_done,task_failed,review_failed docs/plans/example.md
```

The notifier only speaks plain SMTP. `smtps://` is skipped with a warning unless `RALPHTERM_NOTIFY_FORCE_TLS=1` is set, and even then TLS is not supported in the core notifier — front the SMTP server with a TLS proxy if you need encryption in transit.

## Configuration via config file

All CLI flags have config equivalents. Project (`.ralphex/config.json`) overrides global (`~/.config/ralphex/config`).

```json
{
  "notify_telegram_token": "…",
  "notify_telegram_chat": "…",
  "notify_slack_webhook": "https://hooks.slack.example/T/B/X",
  "notify_webhook_url": "http://localhost:9000/ralphterm",
  "notify_email_smtp_url": "smtp://user:pass@mail.example:587",
  "notify_email_from": "ralphterm@example",
  "notify_email_to": "ops@example",
  "notify_on": "plan_done,task_failed,review_failed"
}
```

INI form (global `~/.config/ralphex/config`):

```ini
[notify]
notify_telegram_token = …
notify_telegram_chat_id = …
notify_slack = https://hooks.slack.example/T/B/X
notify_webhook = http://localhost:9000/ralphterm
notify_on = plan_done,task_failed,review_failed
```

Section headers are tolerated for ralphex compatibility but ignored — keys are read from a flat namespace.

## Gotchas

- **No TLS by default.** HTTPS Slack/webhook/Telegram URLs and `smtps://` SMTP URLs are skipped with a warning. Use plain endpoints or front the integration with a local proxy.
- **10-second per-channel timeout.** Slow endpoints will silently fail; check `tracing` logs at `info`/`warn` level for delivery errors.
- **Fire-and-forget.** RalphTerm waits ~500 ms after the run before exiting, so most deliveries complete before the process terminates. Long-tail deliveries may be cut off.
- **Filter precedence.** CLI `--notify-on` overrides config `notify_on` entirely; it is not merged.
- **Empty channels disable notifications.** If no token, URL, or SMTP URL is configured, the notifier is never instantiated and no warnings are emitted.

## Verification

Notification behavior is covered by `tests/notify_compat.rs`, which spins up tiny TCP listeners to capture HTTP and SMTP payloads.
