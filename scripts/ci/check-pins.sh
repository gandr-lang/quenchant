#!/usr/bin/env sh
set -eu

manifest=${WORKSPACE_MANIFEST:-Cargo.toml}
tool_config=${TOOL_CONFIG:-mise.toml}
toolchain_file=${TOOLCHAIN_FILE:-rust-toolchain.toml}

read_table_value() {
  table=$1
  key=$2
  file=$3
  awk -v section="[$table]" -v key="$key" '
    $0 == section { inside = 1; next }
    inside && /^\[/ { exit }
    inside && $0 ~ ("^" key "[[:space:]]*=") {
      value = $0
      sub(/^[^=]*=[[:space:]]*"/, "", value)
      sub(/".*$/, "", value)
      print value
      exit
    }
  ' "$file"
}

read_consumer_library_rev() {
  file=$1
  awk '
    function unquote(value) {
      sub(/^[^=]*=[[:space:]]*"/, "", value)
      sub(/".*$/, "", value)
      return value
    }
    function emit() {
      if (!found && inside && git ~ /github.com\/silvanshade\/quenchant/ &&
          pattern == "crates/quenchant-dylints" && rev != "") {
        print rev
        found = 1
      }
    }
    $0 == "[[workspace.metadata.dylint.libraries]]" {
      emit()
      inside = 1
      git = ""
      rev = ""
      pattern = ""
      next
    }
    inside && /^\[\[/ { emit(); inside = 0 }
    inside && /^git[[:space:]]*=/ { git = unquote($0) }
    inside && /^rev[[:space:]]*=/ { rev = unquote($0) }
    inside && /^pattern[[:space:]]*=/ { pattern = unquote($0) }
    END { emit() }
  ' "$file"
}

consumer_manifest=
consumer_config=
case $# in
  0) ;;
  4)
    if [ "$1" != "--consumer-manifest" ] || [ "$3" != "--consumer-config" ]; then
      echo "usage: $0 [--consumer-manifest Cargo.toml --consumer-config mise.toml]" >&2
      exit 2
    fi
    consumer_manifest=$2
    consumer_config=$4
    ;;
  *)
    echo "usage: $0 [--consumer-manifest Cargo.toml --consumer-config mise.toml]" >&2
    exit 2
    ;;
esac

local_channel=$(read_table_value toolchain channel "$toolchain_file")
clippy_tag=$(read_table_value workspace.dependencies.clippy_utils tag "$manifest")
dylint_linting=$(read_table_value workspace.dependencies.dylint_linting version "$manifest")
dylint_testing=$(read_table_value workspace.dependencies.dylint_testing version "$manifest")
cargo_dylint=$(read_table_value 'tools."cargo:cargo-dylint"' version "$tool_config")
dylint_link=$(sed -n 's/^"cargo:dylint-link"[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$tool_config")

for value in "$local_channel" "$clippy_tag" "$dylint_linting" "$dylint_testing" "$cargo_dylint" "$dylint_link"; do
  if [ -z "$value" ]; then
    echo "pin check FAILED: a required pin is absent" >&2
    exit 1
  fi
done

case $clippy_tag in
  rust-[0-9]*.[0-9]*.[0-9]*) ;;
  *) echo "pin check FAILED: clippy_utils tag is not a stable rust-clippy release tag: $clippy_tag" >&2; exit 1 ;;
esac

remote_toolchain=$(mktemp)
trap 'rm -f "$remote_toolchain"' EXIT HUP INT TERM
curl --fail --silent --show-error --location \
  "https://raw.githubusercontent.com/rust-lang/rust-clippy/${clippy_tag}/rust-toolchain.toml" \
  --output "$remote_toolchain"
remote_channel=$(read_table_value toolchain channel "$remote_toolchain")

if [ "$local_channel" != "$remote_channel" ]; then
  echo "pin check FAILED: local channel $local_channel != $clippy_tag channel $remote_channel" >&2
  exit 1
fi
if [ "$dylint_linting" != "$dylint_testing" ] || [ "$dylint_linting" != "$cargo_dylint" ] || [ "$dylint_linting" != "$dylint_link" ]; then
  echo "pin check FAILED: Dylint crates/tools disagree: linting=$dylint_linting testing=$dylint_testing cargo-dylint=$cargo_dylint dylint-link=$dylint_link" >&2
  exit 1
fi

if [ -n "$consumer_manifest" ]; then
  library_rev=$(read_consumer_library_rev "$consumer_manifest")
  binary_rev=$(sed -n 's/^[[:space:]]*QUENCHANT_REV[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$consumer_config")
  for rev in "$library_rev" "$binary_rev"; do
    case $rev in
      ''|*[!0-9a-f]*)
        echo "pin check FAILED: consumer workflow revisions must be lowercase hexadecimal" >&2
        exit 1
        ;;
    esac
    if [ "${#rev}" -ne 40 ]; then
      echo "pin check FAILED: consumer workflow revisions must be full 40-hex commits" >&2
      exit 1
    fi
  done
  if [ "$library_rev" != "$binary_rev" ]; then
    echo "pin check FAILED: consumer Dylint library rev $library_rev != gate binary rev $binary_rev" >&2
    exit 1
  fi
  echo "pin check OK: consumer Dylint library and gate binary use $library_rev"
fi

echo "pin check OK: $local_channel / $clippy_tag; Dylint $dylint_linting"
