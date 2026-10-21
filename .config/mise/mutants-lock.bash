# One cargo-mutants run per host, sourced by the mutation tasks. A run builds
# several trees at once and holds tens of gigabytes while it does, so two runs
# side by side, each sized for the host alone, exhaust it. `hold_mutants_lock`
# takes `/tmp/gandr-mutants.lock` without waiting and keeps it on file
# descriptor 9 until the task and everything it started have exited. When
# another run holds the lock it refuses, naming that run. A host without
# `flock` (macOS) runs unlocked.

hold_mutants_lock() {
  local lock=/tmp/gandr-mutants.lock
  command -v flock > /dev/null || return 0
  exec 9<> "$lock"
  if ! flock --nonblock 9; then
    printf 'another cargo-mutants run holds %s: %s\n' "$lock" "$(< "$lock")" >&2
    exit 2
  fi
  printf '%s (pid %s, in %s, since %s)\n' "$*" "$$" "$PWD" "$(date -u +%FT%TZ)" > "$lock"
}
