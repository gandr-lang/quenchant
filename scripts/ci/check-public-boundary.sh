#!/usr/bin/env sh
set -eu

failed=0

scan_tree() {
  label=$1
  pattern=$2
  if matches=$(git grep -nIE "$pattern" -- ':/' 2>/dev/null); then
    printf '%s\n' "public-boundary FAILED ($label):" "$matches" >&2
    failed=1
  fi
}

scan_history() {
  label=$1
  pattern=$2
  if matches=$(git log --format=%B --all | grep -nE "$pattern"); then
    printf '%s\n' "public-boundary FAILED in commit messages ($label):" "$matches" >&2
    failed=1
  fi
}

commit_identities() {
  if [ "${GITHUB_EVENT_NAME:-}" = 'pull_request' ] &&
    git rev-parse --verify 'HEAD^2' >/dev/null 2>&1; then
    git log --format='%an%n%ae%n%cn%n%ce' 'HEAD^1' 'HEAD^2'
  else
    git log --format='%an%n%ae%n%cn%n%ce' --all
  fi
}

scan_identities() {
  if matches=$(
    commit_identities |
      awk '
        NR % 2 == 1 {
          name = $0
          next
        }

        {
          if ((name == "agent-shade" || name == "silvanshade") &&
              $0 !~ /@users[.]noreply[.]github[.]com$/) {
            finding = name " uses a non-noreply email address"
            if (!seen[finding]++) {
              print finding
            }
            failed = 1
          }
        }

        END {
          exit failed ? 0 : 1
        }
      '
  ); then
    printf '%s\n' "public-boundary FAILED in commit identities:" "$matches" >&2
    failed=1
  fi
}

if paths=$(git ls-files | grep -E '(^|/)\.(agents|claude|omp)(/|$)'); then
  printf '%s\n' 'public-boundary FAILED (tracked control directories):' "$paths" >&2
  failed=1
fi

home_path='/(Users|home)/[^/[:space:]"<>]+/'
windows_home="[A-Za-z]:\\\\Users\\\\[^\\\\[:space:]\\\"<>]+\\\\"
loopback=local
loopback=${loopback}host
private_host="(^|[^[:alnum:]_.-])((${loopback})|([[:alnum:]-]+\\.)+(local|internal))([^[:alnum:]_.-]|$)"
private_ip='(^|[^0-9])(10\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}|192\.168\.[0-9]{1,3}\.[0-9]{1,3}|172\.(1[6-9]|2[0-9]|3[01])\.[0-9]{1,3}\.[0-9]{1,3})([^0-9]|$)'
internal_uri='(skill|agent|history|artifact|local|omp)://'
credential='(gh[pousr]_[A-Za-z0-9_]{20,}|AKIA[0-9A-Z]{16}|https://[^/[:space:]]+:[^/@[:space:]]+@)'
private_key='^-----BEGIN ([A-Z0-9 ]+ )?PRIVATE KEY-----$'

scan_tree 'home-directory path' "$home_path"
scan_tree 'Windows home-directory path' "$windows_home"
scan_tree 'private hostname' "$private_host"
scan_tree 'private IPv4 address' "$private_ip"
scan_tree 'internal harness URI' "$internal_uri"
scan_tree 'credential-shaped material' "$credential"
scan_tree 'private key' "$private_key"

scan_history 'home-directory path' "$home_path"
scan_history 'Windows home-directory path' "$windows_home"
scan_history 'private hostname' "$private_host"
scan_history 'private IPv4 address' "$private_ip"
scan_history 'internal harness URI' "$internal_uri"
scan_history 'credential-shaped material' "$credential"
scan_history 'private key' "$private_key"

scan_identities

if [ "$failed" -ne 0 ]; then
  exit 1
fi

echo 'public-boundary OK: tracked tree, commit messages, and commit identities contain no refused shapes'
