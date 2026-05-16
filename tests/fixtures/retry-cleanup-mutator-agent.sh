#!/usr/bin/env sh
set -eu
prompt=$(cat)
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
    file-to-dir)
      rm -f baseline-file
      mkdir baseline-file
      printf 'rejected child\n' > baseline-file/rejected-child.txt
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
printf 'COMPLETED\n'
