//! The update path installs nothing it cannot prove came from the release key.
//!
//! ── WHAT THIS FILE IS GUARDING ──
//!
//! `base update` downloads a binary and renames it over the running one. Before
//! signing, the only thing vouching for it was the GitHub URL it came from, so
//! anyone who could answer for that URL — or publish a release on the account —
//! got code execution on every machine that ran a session start.
//!
//! The order is the security property, and it is what these tests measure:
//!
//! 1. `SHA256SUMS` and its signature arrive first.
//! 2. The signature is checked against the key compiled into the binary.
//! 3. Only then is the archive downloaded, and it must hash to the value the
//!    signed checksum file names.
//!
//! The `send` seam is the product's own transport parameter: production passes
//! `|req| req.call()`, these tests pass a closure that answers from memory. No
//! test here opens a socket.
//!
//! The fixture key is a throwaway generated for these tests and committed next
//! to the signature. It is not the release key, and nothing signs a real
//! release with it.

use std::cell::RefCell;

use base::update::{asset_name, fetch_verified_archive_with_key, Release};

const SUMS: &str = include_str!("fixtures/release/SHA256SUMS");
const SIG: &str = include_str!("fixtures/release/SHA256SUMS.minisig");
const ARCHIVE: &[u8] = include_bytes!("fixtures/release/base-linux-x86_64.tar.gz");
/// The fixture public key. It verifies the fixture signature and nothing else.
const TEST_KEY: &str = include_str!("fixtures/release/release-test.pub");

/// A canned 200 whose body is `body`.
fn ok(body: &[u8]) -> ureq::Response {
    let head = format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n", body.len());
    let mut raw = head.into_bytes();
    raw.extend_from_slice(body);
    String::from_utf8(raw)
        .expect("fixtures are UTF-8")
        .parse()
        .expect("canned 200 parses")
}

/// The release the updater would have parsed out of a signed GitHub release.
fn release() -> Release {
    Release {
        tag: "9.9.9".to_string(),
        asset_url: format!("https://example.invalid/{}", asset_name().unwrap()),
        sums_url: "https://example.invalid/SHA256SUMS".to_string(),
        sums_sig_url: "https://example.invalid/SHA256SUMS.minisig".to_string(),
    }
}

/// Answers each of the three downloads from memory and records the order the
/// product asked for them in.
fn transport<'a>(
    seen: &'a RefCell<Vec<String>>,
    sums: &'a [u8],
    sig: &'a [u8],
    archive: &'a [u8],
) -> impl Fn(ureq::Request) -> std::result::Result<ureq::Response, Box<ureq::Error>> + 'a {
    move |req: ureq::Request| {
        let url = req.url().to_string();
        seen.borrow_mut().push(url.clone());
        let body = if url.ends_with("SHA256SUMS.minisig") {
            sig
        } else if url.ends_with("SHA256SUMS") {
            sums
        } else {
            archive
        };
        Ok(ok(body))
    }
}

#[test]
fn a_signed_release_installs_and_the_checksums_are_fetched_first() {
    let dir = tempfile::tempdir().unwrap();
    let seen = RefCell::new(Vec::new());
    let r = release();

    let archive = fetch_verified_archive_with_key(
        &r,
        dir.path(),
        transport(&seen, SUMS.as_bytes(), SIG.as_bytes(), ARCHIVE),
        TEST_KEY,
    )
        .expect("a correctly signed release must verify");

    assert_eq!(std::fs::read(&archive).unwrap(), ARCHIVE);
    let order = seen.into_inner();
    assert_eq!(
        order,
        vec![
            r.sums_url.clone(),
            r.sums_sig_url.clone(),
            r.asset_url.clone()
        ],
        "the signature must be checked before the archive is downloaded"
    );
}

#[test]
fn a_modified_checksum_file_is_refused_before_the_archive_is_fetched() {
    let dir = tempfile::tempdir().unwrap();
    let seen = RefCell::new(Vec::new());
    let r = release();
    let tampered = SUMS.replacen("9c71", "0000", 1);

    let err = fetch_verified_archive_with_key(
        &r,
        dir.path(),
        transport(&seen, tampered.as_bytes(), SIG.as_bytes(), ARCHIVE),
        TEST_KEY,
    )
    .expect_err("a modified SHA256SUMS must not be accepted");

    assert!(
        format!("{err:#}").contains("signature did not verify"),
        "the failure does not name the signature: {err:#}"
    );
    let order = seen.into_inner();
    assert!(
        !order.contains(&r.asset_url),
        "the archive was downloaded even though the signature failed: {order:?}"
    );
}

#[test]
fn a_swapped_archive_is_refused_after_the_signature_passes() {
    let dir = tempfile::tempdir().unwrap();
    let seen = RefCell::new(Vec::new());
    let r = release();

    let err = fetch_verified_archive_with_key(
        &r,
        dir.path(),
        transport(&seen, SUMS.as_bytes(), SIG.as_bytes(), b"a different binary"),
        TEST_KEY,
    )
    .expect_err("the archive does not hash to the signed value");

    assert!(
        format!("{err:#}").contains("refusing to install"),
        "the failure does not say the install was refused: {err:#}"
    );
    assert!(
        seen.into_inner().contains(&r.asset_url),
        "control: this case is only meaningful once the archive was fetched"
    );
}

#[test]
fn a_release_without_a_checksum_file_is_refused_at_the_metadata_step() {
    // No socket, no download: `fetch_latest_release` is the seam that reads the
    // release JSON, and it must reject an unsigned release there.
    let asset = asset_name().unwrap();
    let body = format!(
        r#"{{"tag_name":"v9.9.9","assets":[{{"name":"{asset}","browser_download_url":"https://example.invalid/{asset}"}}]}}"#
    );
    let err = base::update::fetch_latest_release(move |_req| Ok(ok(body.as_bytes())))
        .expect_err("an unsigned release is not installable");
    assert!(
        format!("{err:#}").contains("cannot be verified"),
        "the refusal does not explain itself: {err:#}"
    );
}

#[test]
fn a_release_without_a_signature_file_is_refused_at_the_metadata_step() {
    let asset = asset_name().unwrap();
    let body = format!(
        r#"{{"tag_name":"v9.9.9","assets":[
            {{"name":"{asset}","browser_download_url":"https://example.invalid/{asset}"}},
            {{"name":"SHA256SUMS","browser_download_url":"https://example.invalid/SHA256SUMS"}}
        ]}}"#
    );
    let err = base::update::fetch_latest_release(move |_req| Ok(ok(body.as_bytes())))
        .expect_err("an unsigned release is not installable");
    assert!(
        format!("{err:#}").contains("SHA256SUMS.minisig"),
        "the refusal does not name what is missing: {err:#}"
    );
}
