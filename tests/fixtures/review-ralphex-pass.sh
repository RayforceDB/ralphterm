#!/usr/bin/env sh
set -eu
if [ -n "${RALPHTERM_OUTPUT_FILE:-}" ]; then
  prompt=$(cat "$RALPHTERM_PROMPT_FILE")
  printf '%s\n' "$prompt" > review-prompt.txt
  {
    echo "<<<BEGIN>>>"
    echo "Review: ralphex format"
    echo "<<<RALPHEX:REVIEW_DONE>>>"
    echo "REVIEW_PASS"
    echo "<<<END>>>"
  } > "$RALPHTERM_OUTPUT_FILE"
else
  prompt=$(cat)
  printf '%s\n' "$prompt" > review-prompt.txt
  printf 'Review: ralphex format\n'
  printf '<<<RALPHEX:REVIEW_DONE>>>\n'
fi
