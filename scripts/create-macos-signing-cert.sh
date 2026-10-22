#!/usr/bin/env bash
#
# Create the self-signed certificate Handy Air's macOS builds are signed with,
# and store it as GitHub Actions secrets for .github/workflows/build.yml.
#
# Ad-hoc signed builds look like a new app to macOS after every update, so the
# Accessibility grant stops applying. Signed with one certificate, every build
# has the same designated requirement and the grant carries over.
#
# Run it once, on a Mac, signed in to gh with access to the repository's
# secrets. It also imports the certificate into your login keychain as a
# backup. Keep that copy: a new certificate means every user grants
# Accessibility once more. The certificate is valid for 10 years.
#
# Usage: create-macos-signing-cert.sh [--replace] [owner/repo]
#   --replace   overwrite certificate secrets the repository already has
#   owner/repo  defaults to akshwpsh/Handy-Air
set -euo pipefail

REPLACE=false
if [ "${1:-}" = "--replace" ]; then
  REPLACE=true
  shift
fi
REPO="${1:-akshwpsh/Handy-Air}"
NAME="Handy Air Self-Signed"
# The system LibreSSL writes PKCS#12 that `security import` reads; a Homebrew
# OpenSSL 3 earlier on PATH defaults to ciphers it may not.
OPENSSL=/usr/bin/openssl

if [ "$(uname -s)" != "Darwin" ]; then
  echo "ERROR: run this on a Mac" >&2
  exit 1
fi
gh auth status > /dev/null

SECRETS="$(gh secret list --repo "$REPO" --json name --jq '.[].name')"
if grep -qx MACOS_SELF_SIGNED_CERTIFICATE <<< "$SECRETS" && [ "$REPLACE" != true ]; then
  echo "ERROR: $REPO already has MACOS_SELF_SIGNED_CERTIFICATE." >&2
  echo "Replacing it makes every user grant Accessibility again. Pass --replace to do it anyway." >&2
  exit 1
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

cat > "$TMP/cert.conf" << EOF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $NAME
[ext]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
subjectKeyIdentifier = hash
EOF

"$OPENSSL" req -x509 -newkey rsa:2048 -nodes -sha256 -days 3650 \
  -keyout "$TMP/key.pem" -out "$TMP/cert.pem" -config "$TMP/cert.conf" 2> /dev/null

P12_PASSWORD="$("$OPENSSL" rand -hex 24)"
export P12_PASSWORD
"$OPENSSL" pkcs12 -export -inkey "$TMP/key.pem" -in "$TMP/cert.pem" -name "$NAME" \
  -certpbe PBE-SHA1-3DES -keypbe PBE-SHA1-3DES -macalg sha1 \
  -passout env:P12_PASSWORD -out "$TMP/cert.p12"

security import "$TMP/cert.p12" -k "$HOME/Library/Keychains/login.keychain-db" -f pkcs12 \
  -P "$P12_PASSWORD" -T /usr/bin/codesign

base64 -i "$TMP/cert.p12" | gh secret set MACOS_SELF_SIGNED_CERTIFICATE --repo "$REPO"
printf '%s' "$P12_PASSWORD" | gh secret set MACOS_SELF_SIGNED_CERTIFICATE_PASSWORD --repo "$REPO"

FINGERPRINT="$("$OPENSSL" x509 -in "$TMP/cert.pem" -noout -fingerprint -sha1 | cut -d= -f2)"
echo
echo "Done. \"$NAME\" (SHA-1 $FINGERPRINT)"
echo "  - is stored in $REPO as MACOS_SELF_SIGNED_CERTIFICATE and MACOS_SELF_SIGNED_CERTIFICATE_PASSWORD"
echo "  - is backed up in your login keychain (Keychain Access > login > My Certificates)"
echo "macOS builds from now on are signed with it."
