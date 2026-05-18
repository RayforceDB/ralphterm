#!/usr/bin/env sh
set -eu
if [ -n "${RALPHTERM_OUTPUT_FILE:-}" ]; then
  {
    echo "<<<BEGIN>>>"
    echo "Print REVIEW_PASS only if the task matches the spec and the validation output supports accepting it. Print REVIEW_FAIL with the reason otherwise."
    echo "<<<END>>>"
  } > "$RALPHTERM_OUTPUT_FILE"
else
  cat >/dev/null
  printf 'Print REVIEW_PASS only if the task matches the spec and the validation output supports accepting it. Print REVIEW_FAIL with the reason otherwise.\n'
fi
