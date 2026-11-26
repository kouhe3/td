// Bakes the git commit of this checkout into the crate, mirroring what TDLib does for `commit_hash`
// (configure-time `TD_GIT_COMMIT_HASH` in GitCommitHash.cpp).

use std::process::Command;

fn main() {
    let hash = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_string())
        .unwrap_or_default();
    println!("cargo:rustc-env=TD_GIT_COMMIT_HASH={}", hash);
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs");
}