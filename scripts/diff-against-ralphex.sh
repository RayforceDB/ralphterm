#!/bin/sh
# diff-against-ralphex.sh — verification harness for the ralphex execution-model rewrite.
#
# Usage:
#   scripts/diff-against-ralphex.sh                  # default plan: hello.md, --tasks-only
#   scripts/diff-against-ralphex.sh --full           # also exercise the full review pipeline
#
# Requires:
#   /tmp/ralphex-bin/ralphex          (download from
#     https://github.com/umputun/ralphex/releases/download/v1.2.0/ralphex_1.2.0_linux_amd64.tar.gz
#     and extract into /tmp/ralphex-bin/ if missing)
#   ./target/debug/ralphterm          (built in current repo)
set -eu

REPO_ROOT=$(git rev-parse --show-toplevel)
RALPHTERM_BIN="$REPO_ROOT/target/debug/ralphterm"
RALPHEX_BIN="${RALPHEX_BIN:-/tmp/ralphex-bin/ralphex}"
MODE="--tasks-only"

while [ $# -gt 0 ]; do
  case "$1" in
    --full) MODE="";;
    *) echo "unknown arg: $1" >&2; exit 2;;
  esac
  shift
done

if [ ! -x "$RALPHEX_BIN" ]; then
  echo "MISSING: $RALPHEX_BIN — download from https://github.com/umputun/ralphex/releases/download/v1.2.0/ralphex_1.2.0_linux_amd64.tar.gz" >&2
  exit 1
fi
if [ ! -x "$RALPHTERM_BIN" ]; then
  echo "MISSING: $RALPHTERM_BIN — run \`cargo build\` first" >&2
  exit 1
fi

scratch=$(mktemp -d /tmp/ralphterm-diff-XXXX)
trap 'rm -rf "$scratch"' EXIT

setup_repo() {
  d="$1"
  cd "$d"
  git init -q
  git config user.email t@e.invalid
  git config user.name test
  mkdir -p docs/plans
  cat > docs/plans/hello.md <<'PLAN'
# Hello plan

## Validation Commands
- `test -f hello.txt`

### Task 1: write the file
- [ ] Create a file named hello.txt with the text "hi"
PLAN
  git add -A
  git commit -q -m init
}

# Run ralphex
RX_REPO="$scratch/rx"
mkdir -p "$RX_REPO"
setup_repo "$RX_REPO"
(cd "$RX_REPO" && "$RALPHEX_BIN" --init >/dev/null 2>&1 && git add -A && git commit -q -m "add ralphex config")
(cd "$RX_REPO" && timeout 240 "$RALPHEX_BIN" $MODE docs/plans/hello.md) > "$scratch/rx.out" 2>&1 || true
RX_EXIT=$?

# Run ralphterm
RT_REPO="$scratch/rt"
mkdir -p "$RT_REPO"
setup_repo "$RT_REPO"
(cd "$RT_REPO" && timeout 240 "$RALPHTERM_BIN" $MODE docs/plans/hello.md) > "$scratch/rt.out" 2>&1 || true
RT_EXIT=$?

# Normalise: drop ANSI escapes, timestamps, version banners, commit hashes,
# and tmp paths so the structural diff is meaningful.
normalise() {
  sed -e 's/\x1b\[[0-9;]*[a-zA-Z]//g' \
      -e 's/\[20[0-9][0-9]-[0-9][0-9]-[0-9][0-9] [0-9][0-9]:[0-9][0-9]:[0-9][0-9]\]/[TS]/g' \
      -e 's/^ralph[a-z]* v[^ ]*/<VERSION-BANNER>/' \
      -e 's/[0-9a-f]\{7,40\}/<HASH>/g' \
      -e "s|$1|<REPO>|g" \
      -e 's/completed in [0-9]\+s/completed in <SECS>s/' \
    "$2"
}

normalise "$RX_REPO" "$scratch/rx.out" > "$scratch/rx.norm"
normalise "$RT_REPO" "$scratch/rt.out" > "$scratch/rt.norm"

DIFF=$(diff -u "$scratch/rx.norm" "$scratch/rt.norm" || true)
echo "--- ralphex exit: $RX_EXIT ---"
echo "--- ralphterm exit: $RT_EXIT ---"
echo "--- normalised diff (-=ralphex, +=ralphterm) ---"
if [ -z "$DIFF" ]; then
  echo "OK: transcripts match after normalisation"
  exit 0
fi
echo "$DIFF" | head -120
echo "..."
echo "FAIL: structural divergence detected"
exit 1
