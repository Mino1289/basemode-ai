#!/bin/sh
# Release signing for the fork.
#
#   scripts/release-sign.sh keygen [--ci]   create a keypair, write release-key.pub
#   scripts/release-sign.sh sign <dir>      SHA256SUMS + SHA256SUMS.minisig in <dir>
#   scripts/release-sign.sh verify <dir>    check both, against release-key.pub
#
# `base update` verifies every release with the public key compiled into the
# binary, so the key you sign with and `release-key.pub` must be the same pair.
# `verify` is what proves that before a tag goes out; the release workflow runs
# the same check when MINISIGN_SECRET_KEY is configured.
#
# Requires the `minisign` binary: `apt install minisign`, `brew install minisign`,
# or https://jedisct1.github.io/minisign/.
set -eu

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
PUB="$REPO_ROOT/release-key.pub"
KEY_DIR="${BASE_KEY_DIR:-$HOME/.config/base}"
KEY="${BASE_SIGNING_KEY:-$KEY_DIR/minisign.key}"

# The four assets .github/workflows/release.yml publishes. A release with a
# subset is not a release; checksums are only useful if every download is in
# them.
ASSETS="base-linux-x86_64.tar.gz base-windows-x86_64.zip base-darwin-aarch64.tar.gz base-darwin-x86_64.tar.gz"

say()  { printf '%s\n' "$*"; }
die()  { printf 'release-sign: %s\n' "$*" >&2; exit 1; }

need_minisign() {
  command -v minisign >/dev/null 2>&1 || die "minisign is not on PATH (apt install minisign / brew install minisign)"
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

cmd_keygen() {
  need_minisign
  if [ -e "$KEY" ]; then
    die "$KEY already exists; refusing to overwrite the release key"
  fi
  mkdir -p "$KEY_DIR"
  chmod 700 "$KEY_DIR"

  case "${1:-}" in
    --ci)
      # Unencrypted on purpose: minisign has no way to read a password
      # non-interactively, so a CI signing step cannot use a protected key. The
      # file only ever exists in the secret store and on the runner's tmpfs.
      say "generating an unencrypted keypair (CI signing)"
      minisign -G -W -p "$KEY_DIR/minisign.pub" -s "$KEY"
      ;;
    "")
      say "generating a password-protected keypair"
      minisign -G -p "$KEY_DIR/minisign.pub" -s "$KEY"
      ;;
    *) die "unknown keygen option: $1" ;;
  esac
  chmod 600 "$KEY"

  cp "$KEY_DIR/minisign.pub" "$PUB"
  say ""
  say "wrote $PUB — commit it. Every released binary trusts the key it names."
  say "secret key: $KEY (never commit, never copy into the repo)"
  say ""
  say "next, for CI signing:"
  say "  gh secret set MINISIGN_SECRET_KEY < $KEY"
  say "  # or, to sign by hand after CI builds:"
  say "  scripts/release-sign.sh sign <dir-of-downloaded-assets>"
}

cmd_sign() {
  need_minisign
  dir=${1:-}
  [ -n "$dir" ] || die "usage: $0 sign <dir>"
  [ -d "$dir" ] || die "no such directory: $dir"
  [ -f "$KEY" ] || die "no secret key at $KEY (set BASE_SIGNING_KEY or run keygen)"

  for a in $ASSETS; do
    [ -f "$dir/$a" ] || die "$dir/$a is missing; a release carries all four assets"
  done

  # Fixed order, so the same assets always produce the same file and a diff in
  # SHA256SUMS means a diff in the artifacts.
  : > "$dir/SHA256SUMS"
  for a in $ASSETS; do
    printf '%s  %s\n' "$(sha256_of "$dir/$a")" "$a" >> "$dir/SHA256SUMS"
  done
  minisign -S -s "$KEY" -m "$dir/SHA256SUMS" -x "$dir/SHA256SUMS.minisig"
  say "signed $dir/SHA256SUMS"
}

cmd_verify() {
  need_minisign
  dir=${1:-}
  [ -n "$dir" ] || die "usage: $0 verify <dir>"
  [ -f "$dir/SHA256SUMS" ] || die "$dir/SHA256SUMS is missing"
  [ -f "$dir/SHA256SUMS.minisig" ] || die "$dir/SHA256SUMS.minisig is missing"

  minisign -V -m "$dir/SHA256SUMS" -x "$dir/SHA256SUMS.minisig" -p "$PUB" >/dev/null \
    || die "the signature does not verify against $PUB — wrong key, or the checksums were edited"

  for a in $ASSETS; do
    expected=$(awk -v a="$a" '$2 == a { print $1 }' "$dir/SHA256SUMS")
    [ -n "$expected" ] || die "SHA256SUMS has no entry for $a"
    actual=$(sha256_of "$dir/$a")
    [ "$actual" = "$expected" ] || die "$a does not match SHA256SUMS"
  done
  say "ok: signed by $PUB and all four assets match"
}

case "${1:-}" in
  keygen) shift; cmd_keygen "$@" ;;
  sign)   shift; cmd_sign "$@" ;;
  verify) shift; cmd_verify "$@" ;;
  *) die "usage: $0 keygen [--ci] | sign <dir> | verify <dir>" ;;
esac
