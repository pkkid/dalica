#!/usr/bin/env bash
# SHA256SUMS.txt for a release's artifact set: write it, verify it, and test both.
#
# Written for Dalica (docs/releasing.md, docs/security.md §10); the checksum file is the one the
# Craft apps' release job writes with `sha256sum -- *` (LightCraft .github/workflows/release.yml
# @ 25ef65c8, MIT OR Apache-2.0), so users verify a download with `sha256sum -c` or
# `shasum -a 256 -c` as before.
#
#   packaging/checksums.sh create DIR   write DIR/SHA256SUMS.txt over every other file in DIR
#   packaging/checksums.sh verify DIR   check DIR against DIR/SHA256SUMS.txt (exit 1 on a mismatch)
#   packaging/checksums.sh self-test    plant mismatches in a temporary set and expect `verify`
#                                       to fail on each one (exit 1 if it does not)
#
# `verify` is strict: every listed file must exist with the listed hash, every file in DIR must be
# listed, no name may repeat, and every line must be `<64 hex digits><space><space or *><name>`
# with a plain file name (no `/` or `\`, not `.` or `..`). The artifact set is flat, so DIR must
# not hold sub-folders. Runs on Linux (sha256sum), macOS (shasum) and Windows (Git Bash).
set -euo pipefail

SUMS=SHA256SUMS.txt

die() {
  echo "checksums: $*" >&2
  exit 1
}

# The SHA-256 of one file, as 64 lowercase hex digits.
sha256_of() {
  local out
  if command -v sha256sum >/dev/null 2>&1; then
    out="$(sha256sum -b -- "$1")" || return 1
  elif command -v shasum >/dev/null 2>&1; then
    out="$(shasum -a 256 -b -- "$1")" || return 1
  else
    echo "checksums: neither sha256sum nor shasum is installed" >&2
    return 1
  fi
  out="${out%% *}"
  # GNU sha256sum escapes a name with a backslash or newline by prefixing the hash with `\`.
  printf '%s\n' "${out#\\}"
}

# The artifacts in DIR, one name per line, sorted bytewise; fails on a sub-folder, a link, a name
# with a line break or a backslash, or no files at all.
list_artifacts() {
  local dir="$1" path name out=""
  for path in "$dir"/* "$dir"/.[!.]* "$dir"/..?*; do
    [ -e "$path" ] || [ -L "$path" ] || continue
    name="${path##*/}"
    case "$name" in "$SUMS" | ".$SUMS.tmp") continue ;; esac
    if [ -L "$path" ] || [ ! -f "$path" ]; then
      echo "checksums: $dir/$name is not a regular file (the artifact set is flat)" >&2
      return 1
    fi
    case "$name" in
      *$'\n'* | *$'\r'* | *\\*) echo "checksums: refusing the file name '$name' in $dir" >&2; return 1 ;;
    esac
    out="$out$name"$'\n'
  done
  [ -n "$out" ] || { echo "checksums: $dir holds no artifacts" >&2; return 1; }
  printf '%s' "$out" | LC_ALL=C sort
}

create() {
  local dir="$1" names name hash tmp
  [ -d "$dir" ] || die "$dir is not a folder"
  names="$(list_artifacts "$dir")" || exit 1
  tmp="$dir/.$SUMS.tmp"
  : >"$tmp"
  while IFS= read -r name; do
    hash="$(sha256_of "$dir/$name")" || { rm -f "$tmp"; die "cannot hash $dir/$name"; }
    printf '%s  %s\n' "$hash" "$name" >>"$tmp"
  done <<<"$names"
  mv -f "$tmp" "$dir/$SUMS"
  echo "checksums: wrote $dir/$SUMS ($(wc -l <"$dir/$SUMS" | tr -d ' ') artifacts)"
}

verify() {
  local dir="$1" line name want got bad=0 listed="" names count=0
  local re='^([0-9a-fA-F]{64}) [ *](.+)$'
  [ -d "$dir" ] || die "$dir is not a folder"
  [ -f "$dir/$SUMS" ] || die "$dir/$SUMS is missing"
  while IFS= read -r line || [ -n "$line" ]; do
    line="${line%$'\r'}"
    [ -n "$line" ] || continue
    if [[ ! "$line" =~ $re ]]; then
      echo "checksums: malformed line in $SUMS: $line" >&2
      bad=1
      continue
    fi
    want="$(printf '%s' "${BASH_REMATCH[1]}" | tr 'A-F' 'a-f')"
    name="${BASH_REMATCH[2]}"
    case "$name" in
      */* | *\\* | . | .. | "$SUMS")
        echo "checksums: refusing the name '$name' (plain artifact names only)" >&2
        bad=1
        continue
        ;;
    esac
    if printf '%s' "$listed" | grep -qxF -- "$name"; then
      echo "checksums: $name is listed twice" >&2
      bad=1
      continue
    fi
    listed="$listed$name"$'\n'
    count=$((count + 1))
    if [ -L "$dir/$name" ] || [ ! -f "$dir/$name" ]; then
      echo "checksums: MISSING $name" >&2
      bad=1
      continue
    fi
    got="$(sha256_of "$dir/$name")" || { bad=1; continue; }
    if [ "$got" = "$want" ]; then
      echo "ok  $name"
    else
      echo "checksums: MISMATCH $name (listed $want, file $got)" >&2
      bad=1
    fi
  done <"$dir/$SUMS"
  names="$(list_artifacts "$dir")" || exit 1
  while IFS= read -r name; do
    if ! printf '%s' "$listed" | grep -qxF -- "$name"; then
      echo "checksums: UNLISTED $name (not in $SUMS)" >&2
      bad=1
    fi
  done <<<"$names"
  [ "$count" -gt 0 ] || { echo "checksums: $SUMS lists no artifacts" >&2; bad=1; }
  [ "$bad" = 0 ] || die "verification of $dir FAILED"
  echo "checksums: $count artifacts in $dir match $SUMS"
}

# Each case builds a fresh set, breaks it one way and expects `verify` to refuse it.
self_test() {
  local tmp self case_dir failures=0
  self="${BASH_SOURCE[0]}"
  tmp="$(mktemp -d)"
  # shellcheck disable=SC2064 # expand now: tmp is local
  trap "rm -rf '$tmp'" EXIT

  fresh() {
    case_dir="$tmp/$1"
    mkdir -p "$case_dir"
    printf 'first artifact\n' >"$case_dir/dalica-0.0.0-linux-x86_64.tar.gz"
    printf 'second artifact\n' >"$case_dir/dalica-0.0.0-windows-x64-portable.zip"
    printf 'third, with a space\n' >"$case_dir/dalica cli 0.0.0.zip"
    bash "$self" create "$case_dir" >/dev/null
  }
  expect() { # expect pass|fail CASE
    local out status=0
    out="$(bash "$self" verify "$case_dir" 2>&1)" || status=$?
    if { [ "$1" = pass ] && [ "$status" = 0 ]; } || { [ "$1" = fail ] && [ "$status" != 0 ]; }; then
      echo "self-test ok: $2"
    else
      echo "self-test FAILED: $2 (expected verify to $1, exit $status)" >&2
      printf '%s\n' "$out" | sed 's/^/    /' >&2
      failures=$((failures + 1))
    fi
  }

  fresh clean
  expect pass "an untouched set verifies"
  # The sums file must be readable by the standard tools too.
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$case_dir" && sha256sum -c --quiet "$SUMS") || { echo "self-test FAILED: sha256sum -c" >&2; failures=$((failures + 1)); }
  elif command -v shasum >/dev/null 2>&1; then
    (cd "$case_dir" && shasum -a 256 -c --quiet "$SUMS") || { echo "self-test FAILED: shasum -c" >&2; failures=$((failures + 1)); }
  fi

  fresh changed-file
  printf 'tampered\n' >>"$case_dir/dalica-0.0.0-linux-x86_64.tar.gz"
  expect fail "a changed artifact is a mismatch"

  fresh planted-hash
  # A planted checksum: the right format, the wrong hash (the first artifact's line).
  sed -e '1s/^[0-9a-f]\{64\}/0000000000000000000000000000000000000000000000000000000000000000/' \
    "$case_dir/$SUMS" >"$case_dir/.edit" && mv -f "$case_dir/.edit" "$case_dir/$SUMS"
  expect fail "a planted mismatched checksum"

  fresh missing
  rm -f "$case_dir/dalica-0.0.0-windows-x64-portable.zip"
  expect fail "a listed artifact that is missing"

  fresh unlisted
  printf 'extra\n' >"$case_dir/dalica-0.0.0-macos-universal.zip"
  expect fail "an artifact that is not listed"

  fresh duplicate
  head -n 1 "$case_dir/$SUMS" >>"$case_dir/$SUMS"
  expect fail "a name listed twice"

  fresh malformed
  printf 'not a checksum line\n' >>"$case_dir/$SUMS"
  expect fail "a malformed line"

  fresh traversal
  printf '%s  ../outside\n' "$(sha256_of "$case_dir/$SUMS")" >>"$case_dir/$SUMS"
  expect fail "a name with a path"

  fresh empty-sums
  : >"$case_dir/$SUMS"
  expect fail "an empty checksum file"

  fresh no-sums
  rm -f "$case_dir/$SUMS"
  expect fail "no checksum file"

  mkdir -p "$tmp/empty"
  if bash "$self" create "$tmp/empty" >/dev/null 2>&1; then
    echo "self-test FAILED: create on an empty folder succeeded" >&2
    failures=$((failures + 1))
  else
    echo "self-test ok: create refuses an empty folder"
  fi

  [ "$failures" = 0 ] || die "self-test: $failures case(s) FAILED"
  echo "checksums: self-test passed"
}

case "${1:-}" in
  create) [ $# = 2 ] || die "usage: packaging/checksums.sh create DIR"; create "$2" ;;
  verify) [ $# = 2 ] || die "usage: packaging/checksums.sh verify DIR"; verify "$2" ;;
  self-test) [ $# = 1 ] || die "usage: packaging/checksums.sh self-test"; self_test ;;
  *) die "usage: packaging/checksums.sh create DIR | verify DIR | self-test" ;;
esac
