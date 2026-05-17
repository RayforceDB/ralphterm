# Provider wrappers

RalphTerm drives the OpenAI Codex, GitHub Copilot, Google Gemini, and OpenCode CLIs through small POSIX wrapper scripts. The wrappers translate RalphTerm's PTY-driven loop into the upstream CLI's plain interactive mode, and emit the `COMPLETED` / `FAILED` markers that the orchestrator listens for.

The wrappers ship in `scripts/wrappers/` in this repository and (when installed via the standard layout) at `<exe_dir>/../share/ralphterm/wrappers/`.

## Selecting a provider

You can point `--claude-command` at any wrapper directly:

```
ralphterm --claude-command "$(pwd)/scripts/wrappers/codex.sh" --no-commit plan.md
```

For convenience, the global ralphex-compatible config (`$XDG_CONFIG_HOME/ralphex/config` or `$RALPHEX_CONFIG_DIR/config`) also supports an `[agent]` section. When `claude_command` is not set anywhere else, RalphTerm auto-resolves the bundled wrapper for the requested provider:

```ini
[agent]
provider = codex
```

Supported values for `provider`: `codex`, `copilot`, `gemini`, `opencode`. Unknown values are ignored with a `tracing::warn!` and RalphTerm continues with `claude_command = None` (so an explicit `--claude-command` or an explicit `claude_command =` setting is still required).

## Wrapper contract

Each wrapper:

- Reads a single prompt from stdin and forwards it to the upstream CLI; the remainder of the conversation is driven by the PTY.
- Invokes the upstream CLI **interactively**. The wrappers never use `--print` / `-p` / `--non-interactive` because RalphTerm depends on the streaming TTY output to detect task completion.
- Emits a `COMPLETED` line on exit 0 (the orchestrator also accepts `<<<RALPHEX:ALL_TASKS_DONE>>>`).
- Emits a `FAILED rc=<code>` line on non-zero exit (the orchestrator also accepts `<<<RALPHEX:TASK_FAILED>>>`).
- Forwards `SIGINT` and `SIGTERM` to the child process.
- Forwards `CLAUDE_MODEL` to the provider's model selector when set:
  - codex: `--model`
  - copilot: `--model`
  - gemini: `--model`
  - opencode: `--model`

## Required environment per provider

| Wrapper | Required env | Notes |
| --- | --- | --- |
| `codex.sh` | `OPENAI_API_KEY` | Targets the OpenAI Codex CLI. |
| `copilot.sh` | `GH_TOKEN` (or a prior `gh auth login`) | Targets `gh copilot suggest`. |
| `gemini.sh` | `GEMINI_API_KEY` | Targets Google's Gemini CLI. |
| `opencode.sh` | Provider-specific (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, etc.) | OpenCode picks up its own provider credentials from the environment. |

`ANTHROPIC_API_KEY` is only honoured by the default `claude` integration; it is not used by the wrappers above.

## Override knobs

The wrappers honour a single override env var (`PROVIDER_OVERRIDE`) that swaps the CLI binary name out. This is primarily intended for the wrapper compatibility tests (`tests/wrappers_compat.rs`), which point the wrappers at shim binaries on `$PATH` to verify the I/O contract without invoking the real upstream tools.

## Configuration snippets

### Project-local `.ralphex/config.json`

```json
{
  "claude_command": "/usr/local/share/ralphterm/wrappers/codex.sh"
}
```

When `claude_command` is set, it always wins over `[agent].provider` auto-detection.

### Global `~/.config/ralphex/config`

```ini
[agent]
provider = gemini

[notify]
notify_slack = https://hooks.slack.example/T/B/X
notify_on = plan_done,task_failed
```

When `provider` is set and no `claude_command` exists anywhere, RalphTerm resolves the bundled wrapper for that provider.

### One-off CLI

```bash
# Use the OpenCode wrapper for a single run, ignoring config
ralphterm --tasks-only \
  --claude-command "$(pwd)/scripts/wrappers/opencode.sh" \
  docs/plans/example.md
```

## Combining wrappers with notifications and Docker

The wrapper layer is orthogonal to notifications and Docker isolation. The same command works with both:

```bash
ralphterm --docker --docker-image ralphterm:latest \
  --claude-command "/usr/local/share/ralphterm/wrappers/codex.sh" \
  --notify-slack https://hooks.slack.example/T/B/X \
  --notify-on plan_done,task_failed \
  docs/plans/example.md
```

The wrapper script needs to exist **inside the Docker image** when `--docker` is used. The bundled `docker/Dockerfile` installs the wrappers at `/usr/local/share/ralphterm/wrappers/`, matching the standard installed layout.

## Gotchas

- **Wrappers must be executable.** The repository copies preserve the `+x` bit; if you vendor them into a custom image, set `chmod +x` explicitly.
- **The wrappers do not implement TLS or auth.** They rely on the upstream CLI to handle login and rate limits.
- **`CLAUDE_MODEL` is the lingua franca.** All wrappers translate it into the upstream `--model` flag. Use `--task-model` and `--review-model` to set the variables per role.
- **`--external-review-tool=custom`** with a wrapper-based reviewer is fully supported. Point `--custom-review-script` at any wrapper or shell script that prints `REVIEW_PASS` or `REVIEW_FAIL`.
