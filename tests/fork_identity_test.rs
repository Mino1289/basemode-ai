//! Every place that names the release repo names THIS fork.
//!
//! The failure this guards is quiet and total: a fork that still points at
//! upstream keeps working, then one day `base update` replaces the build with
//! the one it was forked from. `is_current` only compares versions, so the swap
//! happens as soon as upstream tags something newer.
//!
//! Four files hold the coordinate and three of them are not Rust, so a compiler
//! cannot catch a partial edit. This test does, on the source text.

const FORK_REPO: &str = "Mino1289/basemode-ai";

fn read(rel: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

#[test]
fn the_rust_release_coordinates_name_the_fork() {
    assert!(
        read("src/manifest.rs").contains(&format!("GITHUB_REPO: &str = \"{FORK_REPO}\"")),
        "src/manifest.rs must name {FORK_REPO}"
    );
    assert!(
        read("src/update/mod.rs")
            .contains(&format!("https://api.github.com/repos/{FORK_REPO}/releases/latest")),
        "src/update/mod.rs must fetch releases from {FORK_REPO}"
    );
}

#[test]
fn the_installers_name_the_fork() {
    assert!(
        read("install.sh").contains(&format!("REPO=\"{FORK_REPO}\"")),
        "install.sh must download from {FORK_REPO}"
    );
    assert!(
        read("install.ps1").contains(&format!("$repo = '{FORK_REPO}'")),
        "install.ps1 must download from {FORK_REPO}"
    );
}

#[test]
fn no_installer_points_at_upstream() {
    for f in ["install.sh", "install.ps1"] {
        assert!(
            !read(f).contains("ChristopherKahler/base"),
            "{f} still points at upstream, so it would install upstream's binary"
        );
    }
}

/// `install.sh` verifies the release signature with a copy of the public key,
/// and the binary verifies it with `release-key.pub`. Two copies of one key is
/// one copy too many unless something holds them together: a shell script and a
/// Rust constant cannot see each other, and a stale installer key rejects every
/// release the binary accepts (or, worse, trusts a key the binary does not).
#[test]
fn the_installer_trusts_the_same_key_as_the_binary() {
    let key_file = read("release-key.pub");
    let key_line = key_file
        .lines()
        .nth(1)
        .expect("release-key.pub has a key line")
        .trim();
    assert!(!key_line.is_empty(), "release-key.pub has an empty key line");
    assert!(
        read("install.sh").contains(key_line),
        "install.sh does not carry the key line from release-key.pub, so it would \
         reject (or wrongly accept) release signatures"
    );
}
