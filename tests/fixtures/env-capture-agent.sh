#!/usr/bin/env sh
set -eu
prompt=$(cat)
printf '%s\n' "$prompt" > env-capture-last-prompt.txt
printf '%s\n' "${CLAUDE_MODEL-}" > claude-model.txt
printf '%s\n' "${CLAUDE_REVIEW_MODEL-}" > claude-review-model.txt
if printf '%s\n' "$prompt" | grep -q 'Write first.txt'; then
  printf 'created by env capture agent\n' > first.txt
fi
printf 'COMPLETED\n'
