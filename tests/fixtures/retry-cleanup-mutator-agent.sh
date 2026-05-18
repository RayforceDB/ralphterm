#!/usr/bin/env sh
set -eu
if [ -n "${RALPHTERM_OUTPUT_FILE:-}" ]; then
  prompt=$(cat "$RALPHTERM_PROMPT_FILE")
  driver_mode=1
else
  prompt=$(cat)
  driver_mode=0
fi
state_dir=.ralphterm/retry-cleanup-mutator-agent
mkdir -p "$state_dir"
count_file="$state_dir/count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s\n' "$count" > "$count_file"
printf '%s\n' "$count" > agent-count.txt
printf '%s\n' "$prompt" > "$state_dir/prompt-$count.txt"
scenario=${RALPHTERM_RETRY_CLEANUP_SCENARIO:-basic}
if [ "$count" -eq 1 ]; then
  printf 'needs review fix\n' > first.txt
  printf 'rejected attempt only\n' > rejected.txt
  case "$scenario" in
    chmod-executable)
      printf 'changed by rejected attempt\n' > executable.sh
      chmod 0644 executable.sh
      ;;
    chmod-directory)
      chmod 0755 restricted-dir
      ;;
    chmod-non-traversable-directory)
      chmod 000 restricted-dir
      ;;
    file-to-dir)
      rm -f baseline-file
      mkdir baseline-file
      printf 'rejected child\n' > baseline-file/rejected-child.txt
      ;;
    new-non-traversable-directory)
      mkdir rejected-dir
      printf 'rejected child\n' > rejected-dir/rejected-child.txt
      chmod 000 rejected-dir
      ;;
    basic|symlink-survives)
      ;;
    *)
      printf 'unknown scenario: %s\n' "$scenario" >&2
      exit 64
      ;;
  esac
else
  if ! printf '%s\n' "$prompt" | grep -q 'Previous review failed'; then
    printf 'retry prompt missing review feedback\n' >&2
    exit 42
  fi
  printf 'fixed after review\n' > first.txt
fi
if [ "$driver_mode" = "1" ]; then
  {
    echo "<<<BEGIN>>>"
    printf 'retry-cleanup-mutator iteration=%s scenario=%s\n' "$count" "$scenario"
    echo "ALL_TASKS_DONE"
    echo "<<<END>>>"
  } > "$RALPHTERM_OUTPUT_FILE"
else
  printf 'COMPLETED\n'
fi
