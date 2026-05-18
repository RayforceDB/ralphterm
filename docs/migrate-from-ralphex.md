# Migrating from ralphex

RalphTerm is designed to drop into an existing ralphex setup. In most cases you only swap the binary and keep your configuration, plans, prompts, and scripts. This guide walks the migration step by step and flags the differences.

## 1. Install RalphTerm

From source (recommended while the project is pre-1.0):

```bash
git clone git@github.com:RayforceDB/ralphterm.git
cd ralphterm
cargo install --path .
```

This installs two binaries on your `$PATH`:

- `ralphterm` — the canonical name.
- `ralphex` — an alias for `ralphterm` that ships from the same crate.

## 2. Verify both binaries work

```bash
ralphex --version
ralphterm --version
```

Both should print the same version. If `ralphex` is not on your `$PATH`, ensure `$CARGO_HOME/bin` (typically `~/.cargo/bin`) is included.

## 3. Sanity check with `--tasks-only`

Pick an existing plan that you have run with ralphex before. Confirm it still runs:

```bash
ralphterm --tasks-only docs/plans/<your-plan>.md
```

`--tasks-only` skips the review gate, so this is the smallest possible exercise. The implementation agent runs in a real PTY using `claude_command` (from your config) or `claude` (from `$PATH`).

**Workspace-trust precondition.** Unlike ralphex (which used `claude --print` and never saw an interactive REPL), RalphTerm launches `claude` interactively. Claude Code requires per-workspace trust acceptance the first time it sees a directory. RalphTerm refuses to drive an untrusted workspace and prints exactly what to do:

```
ralphterm needs Claude Code to trust this workspace.
Have you run `claude` here once and accepted the trust dialog? [y/N]
```

Run `claude` in the directory once, accept the dialog, then Ctrl+D out and answer `y`. RalphTerm writes `.ralphex/trusted` (a small sentinel file with the acceptance timestamp) so it never asks again for that workspace. In CI / unattended environments, set `RALPHTERM_ASSUME_TRUSTED=1` to skip the check. The first run after migration is therefore one prompt longer than ralphex; subsequent runs are identical.

If you see warnings about unsupported flags or missing config keys, see [ralphex-compat.md](ralphex-compat.md) for the current support matrix.

## 4. Re-enable the review gate

Ralphex's default (full) mode runs implementation **and** review. RalphTerm matches that default — including the `external_review_tool = codex` default — so `ralphterm docs/plans/<your-plan>.md` works out of the box if you have `codex` installed:

```bash
ralphterm docs/plans/<your-plan>.md
```

To pin a different reviewer:

```bash
ralphterm \
  --external-review-tool=custom \
  --custom-review-script "codex exec review-task" \
  docs/plans/<your-plan>.md
```

You can keep these settings in `~/.config/ralphex/config`:

```ini
external_review_tool = custom
custom_review_script = codex exec review-task
```

To disable the review gate (matches `ralphex --tasks-only` semantics) without `--tasks-only`:

```ini
external_review_tool = none
```

## 5. Confirm `.ralphex/` is read

RalphTerm reads both the global ralphex config and per-project `.ralphex/` overrides:

- Global: `~/.config/ralphex/config` (or `$XDG_CONFIG_HOME/ralphex/config`, or `$RALPHEX_CONFIG_DIR/config`).
- Project local: `.ralphex/config.json` (preferred), then `.ralphex/config` (INI fallback).

There is no migration step needed for the global config. For new project overrides, prefer JSON; existing INI files continue to parse.

## 6. Optional: notifications

Notifications are documented in [notifications.md](notifications.md). To enable Slack notifications on plan completion:

```bash
ralphterm --notify-slack https://hooks.slack.example/T/B/X \
  --notify-on plan_done docs/plans/<your-plan>.md
```

## 7. Optional: Docker isolation

See [docker.md](docker.md). To run the agent inside the bundled image:

```bash
ralphterm --docker --docker-image ralphterm:latest \
  --preserve-anthropic-api-key docs/plans/<your-plan>.md
```

## 8. Optional: worktree-isolated runs

```bash
ralphterm --worktree --branch slice/<plan-slug> docs/plans/<your-plan>.md
```

This is the rough equivalent of ralphex's `--worktree` flow. The worktree lives in `.ralphterm/workspaces/<id>` and is not auto-cleaned. Remove it later with `ralphterm workspace cleanup <id>`.

## 9. Optional: alternate providers

Wrappers for Codex, Copilot, Gemini, and OpenCode ship in `scripts/wrappers/`. See [providers.md](providers.md) for the contract and configuration knobs.

## Differences from ralphex

The compatibility matrix in [ralphex-compat.md](ralphex-compat.md) is the authoritative list. The most user-visible differences:

- **Project config format.** RalphTerm prefers `.ralphex/config.json`. An INI `.ralphex/config` file still parses (so existing setups keep working), but new project overrides should use JSON.
- **`--idle-timeout` and `--wait` are accepted but unused.** Scripts that pass them will not fail; they print a warning and the run proceeds without honoring the value.
- **`--base-ref` is accepted without full diff-range support yet.** It is forwarded but does not narrow the review diff.
- **`--init`, `--reset`, `--dump-defaults`, and `--skip-finalize` are not yet implemented.** Passing them produces a clap error.
- **HTTPS notification endpoints are skipped by default.** The built-in notifier is non-TLS to avoid pulling in a heavy crate. Use plain `http://` endpoints, set `RALPHTERM_TELEGRAM_BASE` to a local proxy, or terminate TLS in front of RalphTerm.
- **`-V` is the short form of `--version`.** ralphex uses `-v`; clap reserves `-v` for future verbosity flags.

## Rolling back

If you need to switch back to ralphex temporarily, your `~/.config/ralphex/` directory was never modified. Uninstall the alias and reinstall ralphex:

```bash
cargo uninstall ralphterm   # also removes the ralphex alias
# then reinstall ralphex from upstream
```

Plans, prompts, agents, and configs continue to work in both directions.
