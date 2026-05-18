#!/bin/sh
# RalphTerm one-line installer.
#
# Strategy:
#   1. Download the cargo-dist-generated installer from the latest
#      GitHub Release and try it. If a prebuilt binary exists for the
#      caller's platform, we're done.
#   2. If the installer reports no download for the platform (cargo-dist
#      exits with a recognisable error), fall back to `cargo install
#      ralphterm`. Requires a Rust toolchain; if cargo isn't on PATH we
#      print the install instructions and exit non-zero.
set -eu

INSTALLER_URL="https://github.com/RayforceDB/ralphterm/releases/latest/download/ralphterm-installer.sh"
TMP=$(mktemp)
trap 'rm -f "$TMP"' EXIT INT TERM

if ! curl -fsSL "$INSTALLER_URL" -o "$TMP"; then
  echo "Failed to download $INSTALLER_URL" >&2
  echo "Falling back to: cargo install ralphterm" >&2
  if command -v cargo >/dev/null 2>&1; then
    exec cargo install ralphterm
  fi
  echo "Cargo not found. Install Rust from https://rustup.rs and rerun, or" >&2
  echo "see https://ralphterm.rayforcedb.com/docs/ for manual install options." >&2
  exit 1
fi

# Capture the cargo-dist installer's stdout+stderr so we can decide
# whether to fall back without forcing the user to read both attempts.
OUT=$(mktemp)
trap 'rm -f "$TMP" "$OUT"' EXIT INT TERM
if sh "$TMP" "$@" >"$OUT" 2>&1; then
  cat "$OUT"
  exit 0
fi

# Common cargo-dist failure modes that mean "wrong platform":
if grep -qE "isn't a download for your platform|no precompiled binaries available" "$OUT"; then
  echo "No prebuilt binary for this platform yet." >&2
  echo "Falling back to: cargo install ralphterm" >&2
  if command -v cargo >/dev/null 2>&1; then
    exec cargo install ralphterm
  fi
  echo "Cargo not found. Install Rust from https://rustup.rs and rerun." >&2
  exit 1
fi

# Some other failure — surface the original output and exit non-zero.
cat "$OUT" >&2
exit 1
