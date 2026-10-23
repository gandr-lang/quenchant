# Sourced by `pr:land` and `pr:watch`, after `repo` is set.

# Remove this clone's worktree of pull request `$1`'s branch once it merged,
# in the foreground: `wt remove` deletes the branch as merged and refuses a
# dirty or otherwise unsafe tree, whose refusal is printed and the tree kept.
# The primary worktree is never removed. Prints on stderr only.
#
# `pr:land` and `pr:watch` both run this, usually from the branch's own
# worktree, so whichever sees the merge second finds its working directory
# gone, and every later git command fails ("Unable to read current working
# directory"). The function therefore first moves the shell to the primary
# worktree, read when this file is sourced; the caller exits right after
# (gandr-lang/gandr#2042).
worktree_primary=$(git worktree list --porcelain | sed -n '1s/^worktree //p')
remove_landed_worktree() {
  local branch path out
  cd "$worktree_primary" || return 0
  branch=$(gh pr view "$1" -R "$repo" --json headRefName --jq .headRefName) || return 0
  path=$(git worktree list --porcelain | awk -v ref="branch refs/heads/$branch" '/^worktree / { tree = substr($0, 10) } $0 == ref { print tree }')
  [[ -n "$path" && "$path" != "$worktree_primary" ]] || return 0
  if out=$(wt remove --foreground --yes "$path" 2>&1); then
    echo "removed the worktree of #$1 at $path" >&2
  else
    echo "kept the worktree of #$1 at $path: ${out##*$'\n'}" >&2
  fi
}
