#!/usr/bin/env sh
set -eu
if [ -n "${RALPHTERM_OUTPUT_FILE:-}" ]; then
  prompt=$(cat "$RALPHTERM_PROMPT_FILE")
  printf '%s\n' "$prompt" > review-prompt.txt
  {
    echo "<<<BEGIN>>>"
    echo "Review: pass"
    echo "REVIEW_PASS"
    echo "<<<END>>>"
  } > "$RALPHTERM_OUTPUT_FILE"
else
  prompt=$(cat)
  printf '%s\n' "$prompt" > review-prompt.txt
  printf 'Review: pass\n'
  printf 'REVIEW_PASS\n'
fi
