#!/usr/bin/env sh
set -eu
if [ -n "${RALPHTERM_OUTPUT_FILE:-}" ]; then
  # Drive_agent path: emit an empty BEGIN/END so the run completes
  # with no findings. Decision logic falls through to Some(true).
  {
    echo "<<<BEGIN>>>"
    echo "<<<END>>>"
  } > "$RALPHTERM_OUTPUT_FILE"
else
  cat >/dev/null
fi
