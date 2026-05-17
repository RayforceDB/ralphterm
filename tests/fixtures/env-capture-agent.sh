#!/usr/bin/env sh
# env-capture-agent.sh — like fake-agent.sh but additionally captures the
# CLAUDE_MODEL / CLAUDE_REVIEW_MODEL environment variables to side files.
set -eu

prompt=$(cat)
printf '%s\n' "${CLAUDE_MODEL-}" > claude-model.txt
printf '%s\n' "${CLAUDE_REVIEW_MODEL-}" > claude-review-model.txt

plan_file=$(printf '%s' "$prompt" | grep -oE 'Read the plan file at [^[:space:]]+' | head -1 | sed 's/.*at //' | sed 's/[.,;:]*$//')
if [ -z "${plan_file:-}" ] || [ ! -f "$plan_file" ]; then
  printf 'FAILED: could not locate plan file\n'
  exit 1
fi

task_line=$(grep -nE '^- \[ \]' "$plan_file" | head -1 || true)
if [ -z "$task_line" ]; then
  printf 'ALL_TASKS_DONE\n'
  exit 0
fi

line_num=$(printf '%s' "$task_line" | cut -d: -f1)
task_text=$(printf '%s' "$task_line" | cut -d: -f2-)

if printf '%s' "$task_text" | grep -q 'Write first.txt'; then
  printf 'created by env capture agent\n' > first.txt
fi

tmp=$(mktemp)
awk -v ln="$line_num" 'NR==ln { sub(/- \[ \]/, "- [x]"); print; next } { print }' "$plan_file" > "$tmp"
mv "$tmp" "$plan_file"

remaining=$(grep -cE '^- \[ \]' "$plan_file" || true)
if [ "${remaining:-0}" -eq 0 ]; then
  printf 'ALL_TASKS_DONE\n'
fi
