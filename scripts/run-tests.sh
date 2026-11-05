#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
scratch_root=${TMPDIR:-"$repo_root/.tmp"}
mkdir -p "$scratch_root"
scratch_root=$(cd "$scratch_root" && pwd)
run_dir=$(mktemp -d "$scratch_root/test-run.XXXXXX")
test_pid=

cleanup() {
  local result=$?
  trap - EXIT
  # Output pipes can close during cancellation; finish cleanup regardless.
  trap '' HUP INT TERM PIPE
  set +m
  if [[ -n "$test_pid" ]]; then
    local groups group live
    # Downloaders create their own groups. Capture those while their parents
    # still exist; terminating Cargo first would lose the ancestry information.
    groups=$(ps -e -o pid= -o ppid= -o pgid= | awk -v root="$test_pid" '
      { parent[$1] = $2; group[$1] = $3 }
      END {
        owned[root] = 1
        do {
          changed = 0
          for (pid in parent) {
            if (!owned[pid] && owned[parent[pid]]) {
              owned[pid] = 1
              changed = 1
            }
          }
        } while (changed)
        for (pid in owned) {
          if (owned[group[pid]]) groups[group[pid]] = 1
        }
        groups[root] = 1
        for (id in groups) print id
      }
    ') || { printf 'ERROR: could not inspect test descendants\n' >&2; result=1; groups=$test_pid; }
    for group in $groups; do
      kill -TERM -- "-$group" 2>/dev/null || true
    done
    for ((attempt = 0; attempt < 50; attempt++)); do
      live=0
      for group in $groups; do
        if kill -0 -- "-$group" 2>/dev/null; then live=1; fi
      done
      [[ "$live" == 1 ]] || break
      sleep 0.1
    done
    for group in $groups; do
      kill -KILL -- "-$group" 2>/dev/null || true
    done
    wait "$test_pid" 2>/dev/null || true
  fi
  rm -rf -- "$run_dir" || result=1
  exit "$result"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

export TMPDIR="$run_dir"
set -m
"$@" &
test_pid=$!
wait "$test_pid"
