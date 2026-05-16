#!/usr/bin/env sh
set -eu
prompt=$(cat)
count_file=agent-count.txt
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s\n' "$count" > "$count_file"
printf '%s\n' "$prompt" > "agent-prompt-$count.txt"
if [ "$count" -eq 1 ]; then
  printf 'needs review fix\n' > first.txt
else
  if ! printf '%s\n' "$prompt" | grep -q 'Previous review failed'; then
    printf 'retry prompt missing review feedback\n' >&2
    exit 42
  fi
  printf 'fixed after review\n' > first.txt
fi
printf 'COMPLETED\n'
