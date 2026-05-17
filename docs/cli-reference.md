# CLI Reference

This page documents every flag and subcommand exposed by the `ralphterm` binary. The `ralphex` binary is an alias for `ralphterm` and accepts the same arguments, so every flag here applies to both.

Show the live help with:

```bash
ralphterm --help
ralphex --help
```

## Invocation forms

```text
ralphterm [RALPHEX_FLAGS] [plan-file]
ralphterm run <plan> [RUN_FLAGS]
ralphterm serve [--bind <addr>]
ralphterm smoke [--agent claude|codex | --agent-command <cmd>]
ralphterm workspace create <id>
ralphterm workspace cleanup <id>
```

The first form is the ralphex-compatible front door: it accepts a positional `<plan-file>` and all of the compatibility flags below. The `run`, `serve`, `smoke`, and `workspace` subcommands are the native interface.

## Run modes

| Flag | Short | Type | Default | Description |
| --- | --- | --- | --- | --- |
| `--tasks-only` | `-t` | bool | off | Run only the task phase. Skips the review gate. |
| `--review` | `-r` | bool | off | Skip task execution; run the reviewer pipeline once. Requires `--external-review-tool=custom --custom-review-script <cmd>`. |
| `--external-only` | `-e` | bool | off | Run external review/fixer loop only. |
| `--codex-only` | `-c` | bool | off | Alias for `--external-only`. |
| `--max-iterations <N>` | `-m` | usize | `50` | Ceiling on per-task implementer attempts. |
| `--max-external-iterations <N>` | | usize | unset | Ceiling on external review-loop iterations. |
| `--review-patience <N>` | | usize | `2` | Abort retry loop after N consecutive identical review failures. |
| `--no-commit` | | bool | off | RalphTerm extension. Skip local checkpoint commit after acceptance. |
| `--move-completed` | | bool | off | Move successful plan files into `<plan-dir>/completed/`. |

Mode flags are mutually exclusive: `--tasks-only`, `--review`, `--external-only`, and `--codex-only` can not be combined.

## Agents

| Flag | Type | Default | Description |
| --- | --- | --- | --- |
| `--claude-command <cmd>` | string | from config / `claude` | Implementer command. Runs in a PTY. Never invoked with `-p`/`--print`. |
| `--claude-args <args>` | string | unset | Extra args appended to `--claude-command`. Shell-split. |
| `--task-model <model>` | string | unset | Exported as `$CLAUDE_MODEL` to the agent. |

The `ralphterm run` subcommand uses `--agent claude|codex` or `--agent-command <cmd>` instead.

## Reviewers

| Flag | Type | Default | Description |
| --- | --- | --- | --- |
| `--external-review-tool <value>` | enum | unset | `custom` or `none`. |
| `--custom-review-script <cmd>` | string | unset | Required when `--external-review-tool=custom`. |
| `--review-model <model>` | string | unset | Exported as `$CLAUDE_REVIEW_MODEL` to the reviewer. |
| `--base-ref <ref>` | string (`-b`) | unset | Git ref used as the review diff base. Accepted; full diff-range support pending. |

The `ralphterm run` subcommand uses `--review-agent claude|codex` or `--review-command <cmd>` instead, with `--require-review` to gate.

## Worktrees

| Flag | Type | Default | Description |
| --- | --- | --- | --- |
| `--worktree` | bool | off | Create an isolated git worktree under `.ralphterm/workspaces/<id>`. |
| `--branch <name>` | string | derived from plan filename | Override the worktree branch. Requires `--worktree`. |

The native subcommand uses `ralphterm run --workspace-id <id>` for the same effect.

## Server / dashboard

| Flag | Short | Type | Default | Description |
| --- | --- | --- | --- | --- |
| `--serve` | `-s` | bool | off | Start the dashboard / API server instead of running a plan. |
| `--port <port>` | `-p` | u16 | `7878` | Bind port. Requires `--serve`. `0` picks an OS-assigned port. |
| `--host <addr>` | | string | `127.0.0.1` | Bind address. Requires `--serve`. |
| `--watch <path>` | `-w` | path | none | Announce filesystem path(s) to monitor while serving. Validated but not yet wired into a real watcher. |

The native interface is `ralphterm serve --bind 127.0.0.1:7878`.

## Notifications

| Flag | Type | Description |
| --- | --- | --- |
| `--notify-telegram-token <token>` | string | Telegram bot token. Paired with `--notify-telegram-chat`. |
| `--notify-telegram-chat <id>` | string | Telegram chat id. |
| `--notify-slack <url>` | string | Slack incoming-webhook URL. |
| `--notify-webhook <url>` | string | Generic HTTP webhook URL (POST JSON). |
| `--notify-email-smtp-url <url>` | string | SMTP URL (`smtp://user:pass@host:port`). `smtps://` is skipped — see [notifications](notifications.md). |
| `--notify-email-from <addr>` | string | Email From address. |
| `--notify-email-to <addr>` | string | Email To address. |
| `--notify-on <list>` | comma-separated | Event filter: `plan_done,task_failed,review_failed,rate_limit`. |

## Docker

| Flag | Type | Description |
| --- | --- | --- |
| `--docker` | bool | Run the implementer/reviewer inside a docker container. |
| `--docker-image <image>` | string | Override the image. Default `ralphterm:latest`. Requires `--docker`. |
| `--preserve-anthropic-api-key` | bool | Forward `ANTHROPIC_API_KEY` into the container. Requires `--docker`. |

See [docker.md](docker.md) for setup and gotchas.

## Misc

| Flag | Short | Type | Description |
| --- | --- | --- | --- |
| `--config-dir <path>` | | path | Global config directory. Defaults to `$RALPHEX_CONFIG_DIR` or `$XDG_CONFIG_HOME/ralphex`. |
| `--session-timeout <dur>` | | duration | Per-agent PTY session timeout (e.g. `30s`, `5m`, `1h`). |
| `--idle-timeout <dur>` | | duration | Accepted but not yet implemented. |
| `--wait <dur>` | | duration | Accepted but not yet implemented. |
| `--debug` | `-d` | bool | Sets `RUST_LOG=debug` if unset. |
| `--no-color` | | bool | Sets `NO_COLOR=1`. |
| `--version` | `-V` | bool | Print version and exit. |
| `--help` | `-h` | bool | Print help and exit. |

## Subcommands

### `ralphterm run <plan>`

The native plan-runner interface. Mostly equivalent to the compat front door, but with explicit flags:

| Flag | Type | Default | Description |
| --- | --- | --- | --- |
| `<plan>` | path | required | Plan markdown file. |
| `--agent claude\|codex` | enum | unset | Built-in implementation agent shortcut. Conflicts with `--agent-command`. |
| `--agent-command <cmd>` | string | unset | Raw implementation command. |
| `--review-agent claude\|codex` | enum | unset | Built-in reviewer shortcut. Conflicts with `--review-command`. |
| `--review-command <cmd>` | string | unset | Raw reviewer command. |
| `--require-review` | bool | off | Fail before starting if no reviewer is configured. |
| `--agent-timeout-ms <ms>` | u64 | unset | Per-agent timeout in milliseconds. |
| `--max-review-retries <N>` | usize | `1` | Maximum implementation retries after `REVIEW_FAIL`. |
| `--no-commit` | bool | off | Skip the local checkpoint commit. |
| `--dry-run` | bool | off | Preview the plan without running an agent. |
| `--workspace-id <id>` | string | unset | Run inside a managed git worktree at `.ralphterm/workspaces/<id>`. |

### `ralphterm serve`

| Flag | Type | Default | Description |
| --- | --- | --- | --- |
| `--bind <addr>` | socket addr | `127.0.0.1:7878` | Bind address for the REST + WebSocket API. |

### `ralphterm smoke`

| Flag | Type | Default | Description |
| --- | --- | --- | --- |
| `--agent claude\|codex` | enum | `claude` | Built-in agent to smoke test. Conflicts with `--agent-command`. |
| `--agent-command <cmd>` | string | unset | Raw command for the smoke session. |

### `ralphterm workspace`

| Subcommand | Description |
| --- | --- |
| `create <id>` | Create `.ralphterm/workspaces/<id>` as a managed git worktree. |
| `cleanup <id>` | Remove the workspace directory. |

## Environment variables

See [ralphex-compat.md](ralphex-compat.md#environment-variables) for the full list. Highlights:

- `RALPHEX_CONFIG_DIR` overrides the global config dir.
- `RALPHEX_EXTRA_VOLUMES`, `RALPHEX_EXTRA_ENV`, `TZ`, `AWS_PROFILE`, `AWS_REGION` extend the docker context.
- `ANTHROPIC_API_KEY` is only forwarded into Docker when `--preserve-anthropic-api-key` is set.

## Examples

```bash
# Drop-in ralphex usage
ralphex --tasks-only --claude-command "$(which claude)" --no-commit docs/plans/example.md

# Native run with review gate
ralphterm run docs/plans/example.md --agent claude \
  --require-review \
  --review-command "codex exec review-task"

# Isolated worktree run
ralphterm --worktree --branch slice/docs-rewrite \
  --external-review-tool=custom --custom-review-script "codex exec review-task" \
  docs/plans/example.md

# Notifications
ralphterm --tasks-only --notify-slack https://hooks.slack.example/T/B/X \
  --notify-on plan_done,task_failed docs/plans/example.md

# Docker-isolated
ralphterm --docker --docker-image ralphterm:latest \
  --preserve-anthropic-api-key docs/plans/example.md
```
