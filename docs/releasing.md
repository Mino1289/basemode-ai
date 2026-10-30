# Releasing

A release is four archives, `SHA256SUMS`, and a minisign signature over
`SHA256SUMS`. The public key lives in `release-key.pub` and is compiled into
every binary, so `base update` can prove that a download came from this project
before it renames it over the running executable.

Unsigned releases are not a degraded mode. They are refused.

## One-time setup

```bash
scripts/release-sign.sh keygen          # password-protected, for signing by hand
scripts/release-sign.sh keygen --ci     # unencrypted, for CI signing
```

Either way it writes:

| Path | What it is |
|---|---|
| `release-key.pub` | public key, **commit this** |
| `~/.config/base/minisign.key` | private key, never commit, `chmod 600` |
| `~/.config/base/minisign.pub` | public key, same content as `release-key.pub` |

Then pick one of the two signing models. Both produce identical artifacts; they
differ only in where the private key lives.

### A. CI signs (default)

```bash
gh secret set MINISIGN_SECRET_KEY < ~/.config/base/minisign.key
```

`.github/workflows/release.yml` signs `SHA256SUMS` on the runner and publishes
the release. The key exists in the GitHub secret store and, for a few seconds
per release, in a file on a runner that is destroyed afterwards.

The trade-off is honest: an attacker who takes over the GitHub account can also
read the secret and sign a malicious release. That is still strictly better than
before this phase, when they did not need the secret at all — but it is not the
strongest option.

### B. You sign offline

Do not set `MINISIGN_SECRET_KEY`. CI builds the artifacts, computes
`SHA256SUMS`, and creates the release as a **draft**. Drafts are invisible to
`/releases/latest`, so no installed binary can be offered one. Download the
assets, sign locally, upload, publish:

```bash
scripts/release-sign.sh sign ./dist
scripts/release-sign.sh verify ./dist     # proves the pair matches release-key.pub
gh release upload v0.17.0 ./dist/SHA256SUMS ./dist/SHA256SUMS.minisig
gh release edit v0.17.0 --draft=false
```

This is the stronger model: a GitHub account takeover cannot produce a release
that verifies, because the private key never touches the account.

Either way, `scripts/release-sign.sh verify` before publishing is what catches
the one silent failure that matters — a secret that is not the pair of the
committed public key. CI runs the same probe when the secret is set.

## What the updater checks

`src/update/verify.rs`, in this order:

1. The release JSON must list `SHA256SUMS` and `SHA256SUMS.minisig`. If either is
   missing the update fails before anything is downloaded.
2. The signature over `SHA256SUMS` must verify against the compiled-in key.
3. The archive must hash to the digest `SHA256SUMS` gives for this platform's
   asset name.

A build whose `release-key.pub` is still the keygen placeholder cannot verify
anything and says so instead of skipping the check. That is why the release
workflow refuses to run the build job while the placeholder is committed.

## Verifying a release by hand

```bash
minisign -V -m SHA256SUMS -x SHA256SUMS.minisig -p release-key.pub
sha256sum -c SHA256SUMS
```

`install.sh` does the checksum check always, and the signature check when
`minisign` is on `PATH` (it carries the same public key; a test holds the two
copies together). `install.ps1` checks the checksum only — PowerShell has no
minisign verifier — and the binary verifies every update after that.

## Rotating the key

A binary only trusts the key compiled into it. So a new key must be shipped in a
binary signed by the **old** key:

1. Generate the new keypair into a separate directory, without overwriting the
   old one (`BASE_KEY_DIR=... scripts/release-sign.sh keygen`).
2. Replace `release-key.pub` and update the copy in `install.sh`.
3. Cut one release signed with the **old** key that contains the new public key.
4. Only then point `MINISIGN_SECRET_KEY` at the new private key.

Skipping step 3 strands every installed binary on a key that will never sign
another release; their updates will be refused, which is safe but terminal.

## Failure modes, and what the operator sees

| Situation | Result |
|---|---|
| Release has no `SHA256SUMS` | `base update` refuses before downloading; says the release cannot be verified |
| Signature does not match | refuses, names the signature, archive is never fetched |
| Archive does not match the checksum | refuses, names both digests, binary is not swapped |
| Build has the placeholder key | refuses, tells the operator to install a signed build |
| Background update fails | one line in `~/.base-gbl/update.log`; the session is not interrupted |
