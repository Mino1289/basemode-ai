//! Release authenticity: a minisign signature over `SHA256SUMS`, then the
//! SHA-256 of the archive it names.
//!
//! Before this, the only thing vouching for a downloaded binary was the URL it
//! came from. Whoever could answer for that URL — or publish a release on the
//! GitHub account — got code execution on every machine running `base update`,
//! silently and on a background timer.
//!
//! Two checks, in this order, and both fail closed:
//!
//! 1. `SHA256SUMS` must carry a valid minisign signature from the key compiled
//!    into this binary. Nothing in the checksum file is trusted before that.
//! 2. The downloaded archive must hash to the value `SHA256SUMS` gives for this
//!    platform's asset name.
//!
//! A release with no signature, a signature from another key, a malformed line,
//! a duplicate entry, or no entry for this platform is an error. There is no
//! "warn and continue": an update that cannot be verified is not installed.

use std::path::Path;

use anyhow::{bail, Context, Result};
use minisign_verify::{PublicKey, Signature};
use sha2::{Digest, Sha256};

/// The public half of the release signing key, at the repo root.
///
/// A file rather than a Rust literal on purpose: `scripts/release-sign.sh
/// keygen` can replace it without touching source, and `git log -p
/// release-key.pub` is then the complete history of who could sign a release.
const EMBEDDED_KEY: &str = include_str!("../../release-key.pub");

/// What a build that has not been through keygen carries instead of a key.
/// Named once so the check and the message cannot drift apart.
const PLACEHOLDER: &str = "UNSET";

/// False in a checkout that never ran keygen. `base update` still works, but
/// refuses to install anything it cannot verify — see `parse_key`.
pub fn embedded_key_is_set() -> bool {
    !EMBEDDED_KEY.contains(PLACEHOLDER)
}

/// The compiled-in key, for the one caller that has to pass it along to the
/// test-only seam in the parent module.
pub(crate) fn embedded_key() -> &'static str {
    EMBEDDED_KEY
}

fn parse_key(text: &str) -> Result<PublicKey> {
    if text.contains(PLACEHOLDER) {
        bail!(
            "this build carries no release signing key, so it cannot verify what it \
             downloads. Install a release built after `scripts/release-sign.sh keygen`, \
             or build from source with a real release-key.pub."
        );
    }
    PublicKey::decode(text).context("the embedded release signing key is malformed")
}

/// Check that `sums` is exactly the file the release key signed.
///
/// `key_text` is a parameter rather than the constant so the test-only seam in
/// the parent module can pass a fixture key; production always passes the
/// compiled-in one.
pub(crate) fn verify_sums(key_text: &str, sums: &[u8], signature: &str) -> Result<()> {
    let key = parse_key(key_text)?;
    let sig = Signature::decode(signature).context("the release signature is malformed")?;
    key.verify(sums, &sig, false)
        .context("the SHA256SUMS signature did not verify against the release key")
}

/// The digest `sums` gives for `asset`, in lowercase hex.
///
/// Strict on purpose: a line that is not `<64 hex> <name>`, an entry for the
/// asset that is not a SHA-256, or two entries for the same asset are all
/// errors. A lenient parser is how a file with a blank or truncated line gets
/// treated as "no opinion" instead of as damage.
pub fn checksum_for(sums: &str, asset: &str) -> Result<String> {
    let mut found: Option<String> = None;
    for (i, line) in sums.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let (Some(hash), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
            bail!("SHA256SUMS line {} is malformed: {line:?}", i + 1);
        };
        if !is_sha256(hash) {
            bail!("SHA256SUMS line {} does not hold a SHA-256 digest: {hash:?}", i + 1);
        }
        if name.trim_start_matches('*') != asset {
            continue;
        }
        if found.is_some() {
            bail!("SHA256SUMS names {asset} more than once");
        }
        found = Some(hash.to_ascii_lowercase());
    }
    found.with_context(|| format!("SHA256SUMS has no entry for {asset}"))
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// SHA-256 of a file, in lowercase hex. Streamed: the archive is tens of MB and
/// there is no reason to hold it in memory twice.
pub fn sha256_file(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).with_context(|| format!("reading {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Check `archive` against the signed `sums`, for this platform's asset.
pub fn verify_archive(archive: &Path, sums: &str, asset: &str) -> Result<()> {
    let expected = checksum_for(sums, asset)?;
    let actual = sha256_file(archive)?;
    if actual != expected {
        bail!(
            "the downloaded {asset} does not match the signed SHA256SUMS \
             (expected {expected}, got {actual}); refusing to install it"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: &str = include_str!("../../tests/fixtures/release/release-test.pub");
    const SUMS: &[u8] = include_bytes!("../../tests/fixtures/release/SHA256SUMS");
    const SIG: &str = include_str!("../../tests/fixtures/release/SHA256SUMS.minisig");
    const ARCHIVE: &[u8] = include_bytes!("../../tests/fixtures/release/base-linux-x86_64.tar.gz");

    fn sums_text() -> String {
        String::from_utf8(SUMS.to_vec()).unwrap()
    }

    #[test]
    fn a_signed_checksum_file_verifies() {
        verify_sums(TEST_KEY, SUMS, SIG).expect("the fixture signature is valid");
    }

    #[test]
    fn one_edited_byte_in_the_checksums_breaks_the_signature() {
        let mut tampered = SUMS.to_vec();
        tampered[0] = b'0';
        let err = verify_sums(TEST_KEY, &tampered, SIG)
            .expect_err("a modified SHA256SUMS must not verify");
        assert!(
            format!("{err:#}").contains("did not verify"),
            "the error does not say the signature failed: {err:#}"
        );
    }

    #[test]
    fn a_signature_from_another_key_is_refused() {
        // Same file, a different key: this is what a substituted SHA256SUMS +
        // .minisig pair from an attacker's key looks like. The key is the
        // minisign-verify crate's own test vector, so it is well-formed and
        // simply does not match the fixture signature.
        let err = verify_sums(
            "untrusted comment: not the release key\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3\n",
            SUMS,
            SIG,
        )
        .expect_err("a signature from another key must not verify");
        assert!(format!("{err:#}").contains("did not verify"), "{err:#}");
    }

    #[test]
    fn a_build_without_a_key_refuses_rather_than_skipping_the_check() {
        let err = verify_sums(
            "untrusted comment: placeholder\nUNSET\n",
            SUMS,
            SIG,
        )
        .expect_err("a placeholder key must not be usable");
        assert!(format!("{err:#}").contains("no release signing key"), "{err:#}");
    }

    #[test]
    fn the_checksum_for_this_platform_is_found() {
        let sums = sums_text();
        for asset in [
            "base-linux-x86_64.tar.gz",
            "base-windows-x86_64.zip",
            "base-darwin-aarch64.tar.gz",
            "base-darwin-x86_64.tar.gz",
        ] {
            assert_eq!(
                checksum_for(&sums, asset).unwrap().len(),
                64,
                "{asset} has no 64-hex digest"
            );
        }
    }

    #[test]
    fn a_missing_entry_is_an_error_not_an_empty_answer() {
        let err = checksum_for(&sums_text(), "base-linux-aarch64.tar.gz")
            .expect_err("an unpublished asset has no checksum");
        assert!(format!("{err:#}").contains("no entry"), "{err:#}");
    }

    #[test]
    fn a_duplicate_entry_is_an_error() {
        let sums = format!(
            "{}  base-linux-x86_64.tar.gz\n{}  base-linux-x86_64.tar.gz\n",
            "a".repeat(64),
            "b".repeat(64)
        );
        let err = checksum_for(&sums, "base-linux-x86_64.tar.gz").expect_err("ambiguous entry");
        assert!(format!("{err:#}").contains("more than once"), "{err:#}");
    }

    #[test]
    fn a_line_that_is_not_a_digest_and_a_name_is_an_error() {
        let sums = "this is not a checksum line\n";
        let err = checksum_for(sums, "base-linux-x86_64.tar.gz").expect_err("malformed line");
        assert!(format!("{err:#}").contains("malformed"), "{err:#}");

        let short = format!("{}  base-linux-x86_64.tar.gz\n", "abc");
        let err = checksum_for(&short, "base-linux-x86_64.tar.gz").expect_err("short digest");
        assert!(format!("{err:#}").contains("SHA-256"), "{err:#}");
    }

    #[test]
    fn the_archive_hashes_to_the_signed_value() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("base-linux-x86_64.tar.gz");
        std::fs::write(&path, ARCHIVE).unwrap();
        verify_archive(&path, &sums_text(), "base-linux-x86_64.tar.gz").unwrap();
    }

    #[test]
    fn a_swapped_archive_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("base-linux-x86_64.tar.gz");
        std::fs::write(&path, b"a different binary").unwrap();
        let err = verify_archive(&path, &sums_text(), "base-linux-x86_64.tar.gz")
            .expect_err("the hash does not match");
        assert!(format!("{err:#}").contains("refusing to install"), "{err:#}");
    }

    #[test]
    fn the_embedded_key_is_a_placeholder_until_keygen_runs() {
        // Not a test of the placeholder itself: a guard that fails the day
        // someone commits a real key here would be worse than useless. This
        // records which state the tree is in, so `base update` behaviour is
        // explained rather than mysterious.
        if !embedded_key_is_set() {
            eprintln!(
                "note: release-key.pub is still the keygen placeholder; \
                 `base update` will refuse to install until it is replaced"
            );
        }
    }
}
