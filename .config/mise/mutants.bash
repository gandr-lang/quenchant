# A mutation run's guards on the host, sourced by the mutation tasks: one run
# at a time, and each run's cargo-mutants in a capped scope.
#
# A run builds several trees at once and holds tens of gigabytes while it
# does, so two runs side by side, each sized for the host alone, exhaust it.
# `hold_mutants_lock` takes `/tmp/gandr-mutants.lock` without waiting and
# keeps it on file descriptor 9 until the task and everything it started have
# exited. When another run holds the lock it refuses, naming that run. A host
# without `flock` (macOS) runs unlocked.
#
# A mutant can turn a bounded loop into an unbounded allocation: one test
# binary reached 53 GB of memory and 76 GB of swap in about a minute, and
# swapping the host to its limit made systemd-oomd end the service that
# started the run. `mutants_scope` is the scope wrapper (`scope`, beside this
# file, by absolute path so a task can call it from a copy of the workspace)
# with hard caps:
# - `MemoryMax=16G`: the kernel's OOM killer acts inside the scope at the cap
#   and chooses its largest process, the runaway test, which then fails, so
#   the mutant reads caught. A dispatcher group's build and tests peaked at
#   1.8 GB, and `mutants` building four at once held 3.4 GB.
# - `MemorySwapMax=0`: the scope never swaps, so it reaches the cap in
#   seconds rather than swapping the host first.
# - `OOMPolicy=continue`: systemd's default ends the whole unit on an OOM
#   kill inside it, which would end the run with its one mutant.
# cargo-mutants' own `--timeout` still ends a mutant that loops without
# allocating.

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

mutants_scope=("$(cd "${BASH_SOURCE[0]%/*}" && pwd)/scope" -p MemoryMax=16G -p MemorySwapMax=0 -p OOMPolicy=continue)
