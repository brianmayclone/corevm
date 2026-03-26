use std::process::Command;

fn main() {
    // Build timestamp (UTC, ISO 8601)
    let timestamp = chrono_utc_now();
    println!("cargo:rustc-env=COREVM_BUILD_TIMESTAMP={}", timestamp);

    // Git short SHA (or "unknown" if not in a git repo)
    let git_sha = git_short_sha();
    println!("cargo:rustc-env=COREVM_GIT_SHA={}", git_sha);

    // Rebuild when git HEAD changes (new commits)
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../VERSION");
}

fn chrono_utc_now() -> String {
    Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn git_short_sha() -> String {
    Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
