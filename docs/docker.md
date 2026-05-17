# Docker isolation

RalphTerm can run the implementation and reviewer commands inside a Docker container. This matches ralphex's Docker mode so existing automation that depends on `--docker` keeps working, and it gives you a sandbox when the agent might execute untrusted code.

## What it does

When `--docker` is passed, RalphTerm wraps each agent command in a `docker run` invocation:

```text
docker run --rm -i --tty \
  -v <cwd>:<cwd> -w <cwd> \
  [-e ANTHROPIC_API_KEY] \
  [-v <extra-volume>] [-e <extra-env>] \
  <image> <agent-command> [<args>...]
```

The same wrapping is applied to the reviewer command when one is configured. The wrapped command is handed back to RalphTerm's existing PTY runner, which means the in-container CLI sees a real TTY just like it would on the host.

## Image

The default image is `ralphterm:latest`. Override it with `--docker-image`:

```bash
ralphterm --docker --docker-image ghcr.io/example/ralphterm-runtime:1.2 \
  docs/plans/example.md
```

A minimal `docker/Dockerfile` and `docker/entrypoint.sh` are bundled in the repo so you can build a local image:

```bash
docker build -t ralphterm:latest docker/
```

The bundled image installs the official Claude Code CLI and the optional provider wrappers. Customize it freely — RalphTerm only needs an entrypoint that can run your agent command in interactive mode.

## CLI flags

| Flag | Description |
| --- | --- |
| `--docker` | Enable Docker isolation. |
| `--docker-image <image>` | Override the image. Requires `--docker`. |
| `--preserve-anthropic-api-key` | Forward `ANTHROPIC_API_KEY` into the container. Requires `--docker`. |

## Environment passthrough

The wrapper honors ralphex's volume/env passthrough env vars so existing pipelines work unchanged:

| Env var | Effect |
| --- | --- |
| `RALPHEX_EXTRA_VOLUMES` | Colon-separated `host:container[:ro]` triples added as `-v` mounts. Multiple volumes pack into a single value: `/a:/a:ro:/b:/b`. |
| `RALPHEX_EXTRA_ENV` | Comma-separated entries. `KEY=VAL` sets a literal value; bare `KEY` forwards the current value of the host env var. |
| `TZ` | Forwarded into the container as `TZ=$TZ`. |
| `AWS_PROFILE` | Forwarded as `AWS_PROFILE=$AWS_PROFILE`. |
| `AWS_REGION` | Forwarded as `AWS_REGION=$AWS_REGION`. |
| `ANTHROPIC_API_KEY` | Only forwarded when `--preserve-anthropic-api-key` is set. |

Example with extra mounts and env:

```bash
export RALPHEX_EXTRA_VOLUMES="$HOME/.claude:/root/.claude:/etc/ssl/certs:/etc/ssl/certs:ro"
export RALPHEX_EXTRA_ENV="GITHUB_TOKEN,DEBUG=1"
ralphterm --docker --preserve-anthropic-api-key docs/plans/example.md
```

The host `$HOME/.claude` directory is mounted read-write so Claude Code can persist auth state. The certificate bundle is mounted read-only. `GITHUB_TOKEN` is forwarded with whatever value it currently has; `DEBUG` is set literally to `1`.

## Working directory

The runner mounts the current working directory at the same path inside the container. Tools that use absolute paths see the same layout. The runner sets `-w <cwd>` so the agent starts where you expect.

## Reviewer wrapping

When `--external-review-tool=custom --custom-review-script <cmd>` is combined with `--docker`, the reviewer command is wrapped too. The reviewer sees the same mounts and env passthrough as the implementer.

## Gotchas

- **Image must be present.** `--docker` does not pull the image for you. If it is missing, the agent PTY fails with the Docker daemon's error.
- **`-i --tty` is always passed.** PTY-driven CLIs need a TTY; running RalphTerm under a non-TTY parent (e.g. CI) may need a wrapper that allocates a pseudoterminal.
- **Auth state lives in volumes.** Without an `~/.claude` mount, the in-container Claude Code CLI will prompt for login.
- **Reviewer must exist inside the image.** A `--custom-review-script "codex exec review-task"` needs the `codex` binary inside the image, not just on the host.
- **`docker_available()` check is runtime.** RalphTerm trusts you when you pass `--docker`. Tests that depend on a real Docker daemon are skipped automatically when the binary is absent.

## Verification

Behavior is covered by `tests/docker_compat.rs`. The test suite uses a stub Docker harness when the `docker` binary is not on `$PATH`, and a tiny ubuntu image otherwise.
