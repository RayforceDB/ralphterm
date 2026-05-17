# Ralphex Compatibility

RalphTerm aims to be a drop-in replacement for [ralphex](https://github.com/umputun/ralphex). It accepts the same CLI surface, reads the same configuration files, listens for the same completion/review signals, and emits the same exit codes for the same situations. This page is the authoritative compatibility reference.

## Status legend

| Status | Meaning |
| --- | --- |
| Supported | Behavior matches ralphex closely enough for existing scripts to work unchanged. |
| Accepted (no-op, warned) | Flag/key parses without error, prints a warning, but currently has no effect. Scripts that pass it will not break. |
| Pending | Not yet implemented. May be present as a stub or rejected with a clear error. |

## CLI flags

| Ralphex flag | RalphTerm equivalent | Status | Notes |
| --- | --- | --- | --- |
| `-t`, `--tasks-only` | `-t`, `--tasks-only` | Supported | Skips the review gate. |
| `-r`, `--review` | `-r`, `--review` | Supported | Skip task phase; run review pipeline only. Requires `--external-review-tool=custom --custom-review-script <cmd>`. |
| `-e`, `--external-only` | `-e`, `--external-only` | Supported | Run external review/fixer loop only. |
| `-c`, `--codex-only` | `-c`, `--codex-only` | Supported | Alias for `--external-only`. |
| `-m`, `--max-iterations` | `-m`, `--max-iterations` | Supported | Default `50`. Stored on `RunOptions`. |
| `--max-external-iterations` | `--max-external-iterations` | Supported | External-loop ceiling. |
| `--review-patience` | `--review-patience` | Supported | Default `2`. Abort after N consecutive identical review failures. |
| `--task-model` | `--task-model` | Supported | Exported as `$CLAUDE_MODEL` to the agent. |
| `--review-model` | `--review-model` | Supported | Exported as `$CLAUDE_REVIEW_MODEL` to the reviewer. |
| `--claude-command` | `--claude-command` | Supported | PTY command, never `-p`/`--print`. |
| `--claude-args` | `--claude-args` | Supported | Shell-split; appended to `--claude-command`. |
| `--external-review-tool` | `--external-review-tool` | Supported (`custom`, `none`) | `codex` value is currently rejected — pass `custom` with `--custom-review-script "codex …"`. |
| `--custom-review-script` | `--custom-review-script` | Supported | Required by `--external-review-tool=custom`. |
| `-b`, `--base-ref` | `-b`, `--base-ref` | Accepted (no-op, warned) | Stored, but full diff-range support is pending. |
| `--session-timeout` | `--session-timeout` | Supported | Parsed (e.g. `30s`, `5m`, `1h`) and applied per agent session. |
| `--idle-timeout` | `--idle-timeout` | Accepted (no-op, warned) | Parsed but currently unused. |
| `--wait` | `--wait` | Accepted (no-op, warned) | Parsed but currently unused. |
| `--skip-finalize` | — | Pending | Not yet recognised. |
| `--preserve-anthropic-api-key` | `--preserve-anthropic-api-key` | Supported | Requires `--docker`. Forwards `ANTHROPIC_API_KEY`. |
| `--worktree` | `--worktree` | Supported | Creates isolated git worktree. |
| `--branch` | `--branch` | Supported | Overrides worktree branch name. Requires `--worktree`. |
| `--plan` | positional `<plan-file>` | Supported | RalphTerm takes the plan as a positional argument. |
| `-d`, `--debug` | `-d`, `--debug` | Supported | Sets `RUST_LOG=debug` if unset. |
| `--no-color` | `--no-color` | Supported | Sets `NO_COLOR=1`. |
| `-v`, `--version` | `-V`, `--version` | Supported | clap emits `-V` short, `--version` long. |
| `-s`, `--serve` | `-s`, `--serve` | Supported | Starts the dashboard server in compat mode. |
| `-p`, `--port` | `-p`, `--port` | Supported | Default `7878`. Requires `--serve`. |
| `--host` | `--host` | Supported | Default `127.0.0.1`. Requires `--serve`. |
| `-w`, `--watch` | `-w`, `--watch` | Supported (announce-only) | Paths are validated and announced; filesystem watching is pending. |
| `--init` | — | Pending | Not yet implemented. |
| `--reset` | — | Pending | Not yet implemented. |
| `--dump-defaults` | — | Pending | Not yet implemented. |
| `--config-dir` | `--config-dir` | Supported | Also reads `RALPHEX_CONFIG_DIR`. |
| `--move-completed` | `--move-completed` | Supported | Moves successful plans to `<plan-dir>/completed/`. |

RalphTerm also exposes flags ralphex does not:

| RalphTerm flag | Purpose |
| --- | --- |
| `--no-commit` | Skip local checkpoint commit after acceptance. |
| `--notify-telegram-token`, `--notify-telegram-chat` | Telegram notifications. |
| `--notify-slack` | Slack incoming-webhook URL. |
| `--notify-webhook` | Generic HTTP webhook URL. |
| `--notify-email-smtp-url`, `--notify-email-from`, `--notify-email-to` | SMTP email notifications. |
| `--notify-on` | Event filter (`plan_done,task_failed,review_failed,rate_limit`). |
| `--docker`, `--docker-image` | Run the implementer/reviewer in a container. |

## Environment variables

| Variable | Status | Notes |
| --- | --- | --- |
| `RALPHEX_CONFIG_DIR` | Supported | Override global config dir. |
| `RALPHEX_EXTRA_VOLUMES` | Supported | Colon-separated `host:container[:ro]` triples for `--docker`. |
| `RALPHEX_EXTRA_ENV` | Supported | Comma-separated `KEY=VALUE` or `KEY` entries forwarded into Docker. |
| `TZ` | Supported | Forwarded into Docker container. |
| `AWS_PROFILE` | Supported | Forwarded into Docker container. |
| `AWS_REGION` | Supported | Forwarded into Docker container. |
| `ANTHROPIC_API_KEY` | Supported (gated) | Only forwarded into Docker when `--preserve-anthropic-api-key` is set. |
| `CLAUDE_MODEL` | Supported | Forwarded by wrappers and exported by `--task-model`. |
| `CLAUDE_REVIEW_MODEL` | Supported | Exported by `--review-model`. |
| `RUST_LOG` | Supported | Standard tracing-subscriber filter. `--debug` defaults it to `debug`. |
| `NO_COLOR` | Supported | Set by `--no-color`. |
| `RALPHTERM_TELEGRAM_BASE` | RalphTerm extension | Override Telegram base URL (testing). |
| `RALPHTERM_NOTIFY_FORCE_TLS` | RalphTerm extension | Attempt TLS endpoints even though the core notifier does not implement TLS. |

## Config file format

RalphTerm reads:

- Global: `~/.config/ralphex/config` (or `$XDG_CONFIG_HOME/ralphex/config`, or `$RALPHEX_CONFIG_DIR/config`).
- Project local: `.ralphex/config.json` first, then `.ralphex/config` (INI) as a fallback.

Project values override global values per field. INI section headers are tolerated but flattened into a single namespace.

Known keys:

| Key | Type | Notes |
| --- | --- | --- |
| `claude_command` | string | Implementer command. |
| `claude_args` | string | Shell-split extra args. |
| `external_review_tool` | string | `custom` or `none`. |
| `custom_review_script` | string | Reviewer command. |
| `max_iterations` | int | |
| `max_external_iterations` | int | |
| `review_patience` | int | |
| `task_model` | string | |
| `review_model` | string | |
| `session_timeout` | duration string | `30s`, `5m`, `1h`. |
| `idle_timeout` | duration string | Parsed; currently unused. |
| `wait` | duration string | Parsed; currently unused. |
| `base_ref` | string | |
| `move_plan_on_completion` | bool | |
| `notify_telegram_token`, `notify_telegram_chat`, `notify_telegram_base` | string | |
| `notify_slack` / `notify_slack_webhook` | string | |
| `notify_webhook` / `notify_webhook_url` | string | |
| `notify_email_smtp_url`, `notify_email_from`, `notify_email_to` | string | |
| `notify_on` | comma-separated string | Filter for delivered events. |

**Difference from ralphex:** local project overrides are JSON, not INI. A `.ralphex/config` INI file still parses as a fallback so existing project setups keep working, but `.ralphex/config.json` is preferred.

## Signal protocol

RalphTerm accepts both the ralphex signal forms and the RalphTerm-native ones:

| Signal | Form |
| --- | --- |
| Completed | `COMPLETED`, `ALL_TASKS_DONE`, `<<<RALPHEX:ALL_TASKS_DONE>>>`, `RALPHEX:ALL_TASKS_DONE` |
| Failed | `FAILED`, `<<<RALPHEX:TASK_FAILED>>>`, `RALPHEX:TASK_FAILED` |
| Review done | `REVIEW_DONE`, `<<<RALPHEX:REVIEW_DONE>>>`, `<<<RALPHEX:CODEX_REVIEW_DONE>>>` |
| Plan ready | `PLAN_READY` |
| Question | `QUESTION` |
| Review pass (decision) | `REVIEW_PASS` |
| Review fail (decision) | `REVIEW_FAIL` |

`REVIEW_DONE` reports that an iteration found zero issues. It is not the same as acceptance; acceptance still requires validation to pass and the review gate to be configured.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Plan accepted (or `--dry-run` preview succeeded). |
| Non-zero | Validation, review, or implementation failed; plan checkbox not marked; commit skipped. |

The non-zero codes follow Rust's default `anyhow` error propagation (`1`). RalphTerm intentionally keeps semantics simple: success is `0`, every failure is non-zero.

## Compatibility tests

The behaviors documented here are covered by integration tests:

- `tests/cli_flag_compat.rs`
- `tests/config_compat.rs`
- `tests/signals_compat.rs`
- `tests/mode_compat.rs`
- `tests/worktree_compat.rs`
- `tests/progress_compat.rs`
- `tests/plan_completion_compat.rs`
- `tests/notify_compat.rs`
- `tests/docker_compat.rs`
- `tests/ralphex_alias.rs`
- `tests/wrappers_compat.rs`
