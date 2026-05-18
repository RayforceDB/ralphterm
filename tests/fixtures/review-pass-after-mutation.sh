#!/usr/bin/env sh
set -eu
if [ -n "${RALPHTERM_OUTPUT_FILE:-}" ]; then
  prompt=$(cat "$RALPHTERM_PROMPT_FILE")
  driver_mode=1
else
  prompt=$(cat)
  driver_mode=0
fi
printf '%s\n' "$prompt" > review-prompt.txt
if [ -f external-state.txt ]; then
  reason="Review: state present"
  verdict="REVIEW_PASS"
else
  reason="Review: needs mutation"
  verdict="REVIEW_FAIL"
fi
if [ "$driver_mode" = "1" ]; then
  {
    echo "<<<BEGIN>>>"
    printf '%s\n' "$reason"
    printf '%s\n' "$verdict"
    echo "<<<END>>>"
  } > "$RALPHTERM_OUTPUT_FILE"
else
  printf '%s\n' "$reason"
  printf '%s\n' "$verdict"
fi
