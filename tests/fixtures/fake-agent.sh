#!/usr/bin/env sh
# fake-agent.sh — simulates an agent that follows ralphex's task.txt
# instructions: read the plan file, find the first unchecked task,
# perform a small recipe based on the task body, mark the checkbox done,
# emit ALL_TASKS_DONE when no unchecked boxes remain.
set -eu

prompt=$(cat)
plan_file=$(printf '%s' "$prompt" | grep -oE 'Read the plan file at [^[:space:]]+' | head -1 | sed 's/.*at //')
if [ -z "${plan_file:-}" ]; then
  if [ -n "${1:-}" ]; then
    prompt="$1"
    plan_file=$(printf '%s' "$prompt" | grep -oE 'Read the plan file at [^[:space:]]+' | head -1 | sed 's/.*at //')
  fi
fi
if [ -z "${plan_file:-}" ]; then
  printf 'FAILED: could not find plan path in prompt\n'
  exit 1
fi
if [ ! -f "$plan_file" ]; then
  printf 'FAILED: plan file does not exist: %s\n' "$plan_file"
  exit 1
fi

# Find the first unchecked task line and perform its recipe.
task_line=$(grep -nE '^- \[ \]' "$plan_file" | head -1 || true)
if [ -z "$task_line" ]; then
  printf '\nAll checkboxes in the plan are now complete.\n'
  printf 'ALL_TASKS_DONE\n'
  exit 0
fi

line_num=$(printf '%s' "$task_line" | cut -d: -f1)
task_text=$(printf '%s' "$task_line" | cut -d: -f2-)

if printf '%s' "$task_text" | grep -q 'Write first.txt'; then
  printf 'created by fake agent\n' > first.txt
elif printf '%s' "$task_text" | grep -q 'Write second.txt'; then
  printf 'created by fake agent\n' > second.txt
elif printf '%s' "$task_text" | grep -q 'Write nested/generated.txt'; then
  mkdir -p nested
  printf 'nested content from fake agent\n' > nested/generated.txt
elif printf '%s' "$task_text" | grep -q 'Change tracked.txt'; then
  printf 'run-change\n' > tracked.txt
elif printf '%s' "$task_text" | grep -q 'Recreate tracked.txt with base content'; then
  printf 'base\n' > tracked.txt
elif printf '%s' "$task_text" | grep -q 'Create a file named hello.txt'; then
  printf 'hi' > hello.txt
fi

tmp=$(mktemp)
awk -v ln="$line_num" 'NR==ln { sub(/- \[ \]/, "- [x]"); print; next } { print }' "$plan_file" > "$tmp"
mv "$tmp" "$plan_file"

remaining=$(grep -cE '^- \[ \]' "$plan_file" || true)
printf '\nMarked task at line %s as complete.\n' "$line_num"
if [ "${remaining:-0}" -eq 0 ]; then
  printf 'All checkboxes in the plan are now complete.\n'
  printf 'ALL_TASKS_DONE\n'
fi
