#!/usr/bin/env bash
# CI only (the `sign macOS universal` job, docs/releasing.md → macOS): put a code-signing identity
# into a temporary keychain so codesign can use it, and delete that keychain again.
#
# Usage: packaging/macos/import-cert.sh               the Developer ID certificate (a release run)
#        packaging/macos/import-cert.sh --throwaway   a self-signed identity made here (a dry run)
#        packaging/macos/import-cert.sh --delete      remove the keychain and the throwaway's trust
#                                                     (the job's last step, whatever happened)
#
# Inputs (env): APPLE_CERTIFICATE (base64 .p12 of a "Developer ID Application" identity),
# APPLE_CERTIFICATE_PASSWORD, KEYCHAIN_PASSWORD (optional; random if unset), RUNNER_TEMP (or
# TMPDIR). --throwaway reads none of them.
# Outputs: MACOS_SIGN_IDENTITY (the identity's SHA-1) and MACOS_KEYCHAIN for package.sh, and
# MACOS_SIGN_THROWAWAY=1 with --throwaway; appended to $GITHUB_ENV in Actions, otherwise printed.
#
# No certificate: a warning and exit 0, so package.sh falls back to ad hoc signing.
#
# --throwaway makes an RSA key and a self-signed certificate for code signing, valid for one day,
# imports them like the real certificate and trusts the certificate for code signing on this
# machine only (admin trust settings, so it needs sudo), so the release workflow's dry runs sign
# with the hardened runtime and a timestamp exactly as a release does. Its signatures are never
# notarized or published. It refuses to run outside GitHub Actions, so it never changes the trust
# settings of a developer's Mac; --delete deletes the keychain and removes the trust again (bounded
# in time: a trust setting it cannot remove without a prompt stays on the discarded runner).
#
# Adapted from PhotoCraft packaging/macos/import-cert.sh @ 77221725 (MIT OR Apache-2.0) and the
# keychain removal step of craft-common-actions .github/workflows/release.yml @ d7bf8f06
# (github.com/storytold/craft-common-actions, MIT OR Apache-2.0). Changed for Dalica: the throwaway
# identity and --delete.
set -euo pipefail
umask 077
# shellcheck source=common.sh
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"

TMP="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
KEYCHAIN="$TMP/dalica-signing.keychain-db"
CERT="$TMP/dalica-signing.p12"
# The throwaway's certificate stays until --delete, which needs it to remove the trust setting.
THROWAWAY_PEM="$TMP/dalica-throwaway.pem"
THROWAWAY_NAME="Dalica dry run (throwaway)"

MODE=release
case "${1:-}" in
  "") ;;
  --throwaway) MODE=throwaway ;;
  --delete) MODE=delete ;;
  -h | --help) sed -n '2,24p' "$0"; exit 0 ;;
  *) echo "unknown argument: $1" >&2; exit 2 ;;
esac

# bounded SECONDS CMD...: CMD, stopped after SECONDS (macOS has no timeout(1)); status 124 then.
# The cleanup must never hang on a prompt, whatever state the job left the keychain in.
bounded() {
  local secs="$1" pid waited=0
  shift
  "$@" </dev/null &
  pid=$!
  while kill -0 "$pid" 2>/dev/null; do
    if [ "$waited" -ge "$secs" ]; then
      kill "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
      return 124
    fi
    sleep 1
    waited=$((waited + 1))
  done
  wait "$pid"
}

if [ "$MODE" = delete ]; then
  if [ -e "$KEYCHAIN" ]; then
    # delete-keychain also takes it off the search list; the throwaway's key and certificate go
    # with it.
    if bounded 60 security delete-keychain "$KEYCHAIN"; then
      echo "deleted $KEYCHAIN"
    else
      rm -f "$KEYCHAIN"
      warn "security delete-keychain failed or hung; removed the file $KEYCHAIN"
    fi
  else
    echo "no temporary keychain to delete"
  fi
  if [ -f "$THROWAWAY_PEM" ]; then
    # Removing an admin trust setting can ask for authorization: never wait for that. A setting
    # left behind names a deleted certificate that expires within a day, on a discarded runner.
    if bounded 30 sudo -n security remove-trusted-cert -d "$THROWAWAY_PEM"; then
      echo "removed the throwaway certificate's trust setting"
    else
      warn "the throwaway certificate's trust setting stays on this runner (its certificate is deleted and expires within a day)"
    fi
    rm -f "$THROWAWAY_PEM"
  fi
  rm -f "$CERT"
  exit 0
fi

KC_PASS="${KEYCHAIN_PASSWORD:-$(openssl rand -hex 24)}"
trap 'rm -f "$CERT"' EXIT

if [ "$MODE" = throwaway ]; then
  if [ "${GITHUB_ACTIONS:-}" != true ]; then
    echo "error: --throwaway changes this machine's trust settings and runs only in GitHub Actions" >&2
    exit 1
  fi
  echo "==> making the throwaway signing identity '$THROWAWAY_NAME' (valid for a day, trusted on this runner only)"
  work="$(mktemp -d "$TMP/dalica-throwaway.XXXXXX")"
  cat >"$work/req.cnf" <<EOF
[req]
distinguished_name = dn
prompt = no
x509_extensions = ext
[dn]
CN = $THROWAWAY_NAME
[ext]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
subjectKeyIdentifier = hash
EOF
  # The system's LibreSSL: its PKCS#12 defaults (3DES, SHA-1 MAC) are what `security import` reads.
  /usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 1 -config "$work/req.cnf" \
    -keyout "$work/key.pem" -out "$THROWAWAY_PEM"
  APPLE_CERTIFICATE_PASSWORD="$(openssl rand -hex 16)"
  /usr/bin/openssl pkcs12 -export -inkey "$work/key.pem" -in "$THROWAWAY_PEM" -name "$THROWAWAY_NAME" \
    -out "$CERT" -passout "pass:$APPLE_CERTIFICATE_PASSWORD"
  rm -rf "$work"
else
  if [ -z "${APPLE_CERTIFICATE:-}" ]; then
    warn "APPLE_CERTIFICATE is not set: the macOS artifacts are signed ad hoc and not notarized"
    exit 0
  fi
  printf '%s' "$APPLE_CERTIFICATE" | base64 --decode >"$CERT"
fi

security create-keychain -p "$KC_PASS" "$KEYCHAIN"
# From here on the keychain exists: --delete removes it even if a later step fails.
security set-keychain-settings -lut 21600 "$KEYCHAIN"
security unlock-keychain -p "$KC_PASS" "$KEYCHAIN"
security import "$CERT" -P "${APPLE_CERTIFICATE_PASSWORD:-}" -T /usr/bin/codesign -T /usr/bin/security -t cert -f pkcs12 -k "$KEYCHAIN"
rm -f "$CERT"
# Let codesign use the key without a UI prompt.
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$KC_PASS" "$KEYCHAIN" >/dev/null
# Put it on the search list (keeping the login keychain) so codesign and notarytool find it.
# shellcheck disable=SC2046
security list-keychains -d user -s "$KEYCHAIN" $(security list-keychains -d user | tr -d '"')

if [ "$MODE" = throwaway ]; then
  # Trusted for code signing in the admin domain: valid for codesign on this runner, nowhere else.
  sudo -n security add-trusted-cert -d -r trustRoot -p codeSign -k "$KEYCHAIN" "$THROWAWAY_PEM"
  pattern="\"$THROWAWAY_NAME\""
else
  pattern='"Developer ID Application:'
fi

IDENTITY="$(security find-identity -v -p codesigning "$KEYCHAIN" | awk -v p="$pattern" 'index($0, p) { print $2; exit }')"
if [ -z "$IDENTITY" ]; then
  security find-identity -p codesigning "$KEYCHAIN" >&2
  if [ "$MODE" = throwaway ]; then
    echo "error: the throwaway identity is not a valid code-signing identity" >&2
  else
    echo "error: no 'Developer ID Application' identity in APPLE_CERTIFICATE" >&2
  fi
  exit 1
fi
echo "Imported signing identity $IDENTITY ($MODE)"
out=("MACOS_SIGN_IDENTITY=$IDENTITY" "MACOS_KEYCHAIN=$KEYCHAIN")
if [ "$MODE" = throwaway ]; then out+=("MACOS_SIGN_THROWAWAY=1"); fi
if [ -n "${GITHUB_ENV:-}" ]; then
  printf '%s\n' "${out[@]}" >>"$GITHUB_ENV"
else
  printf '%s\n' "${out[@]}"
fi
