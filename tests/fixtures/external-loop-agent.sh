#!/usr/bin/env sh
set -eu
prompt=$(cat)
state_dir=.ralphterm/external-loop-agent
mkdir -p "$state_dir"
count_file="$state_dir/count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s\n' "$count" > "$count_file"
printf '%s\n' "$count" > external-agent-count.txt
printf '%s\n' "$prompt" > "external-agent-prompt-$count.txt"
printf 'iteration %s mutation\n' "$count" > external-state.txt
printf 'COMPLETED\n'
