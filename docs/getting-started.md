# Getting Started

## Install from source

```bash
git clone git@github.com:RayforceDB/ralphterm.git
cd ralphterm
cargo build --release
```

## Start the daemon

```bash
cargo run -- serve --bind 127.0.0.1:7878
```

## Check health

```bash
curl http://127.0.0.1:7878/health
```

Expected:

```json
{"ok":true}
```

## Run a deterministic test session

```bash
ID=$(curl -sS -X POST http://127.0.0.1:7878/v1/sessions \
  -H 'content-type: application/json' \
  -d '{
    "agent":"claude",
    "command":"/bin/sh",
    "args":["-lc","read line; printf \"%s\\n\" \"$line\"; echo COMPLETED"],
    "prompt":"hello from ralphterm"
  }' | python3 -c 'import sys,json; print(json.load(sys.stdin)["id"])')

curl http://127.0.0.1:7878/v1/sessions/$ID
curl http://127.0.0.1:7878/v1/sessions/$ID/transcript
```

## Run with real CLIs

Install and authenticate the official tools first:

```bash
claude auth login
codex login
```

Then create a session without `command` override:

```bash
curl -sS -X POST http://127.0.0.1:7878/v1/sessions \
  -H 'content-type: application/json' \
  -d '{"agent":"claude","prompt":"Say hello and end with COMPLETED"}'
```

## Development checks

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```
