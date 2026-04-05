fn main() {
    let mut hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    #[allow(clippy::collapsible_if)]
    if hash.is_empty() {
        if let Ok(content) = std::fs::read_to_string("BUILD_INFO") {
            for line in content.lines() {
                if let Some(stripped) = line.strip_prefix("commit:") {
                    let full_hash = stripped.trim();
                    hash = full_hash.chars().take(7).collect();
                    break;
                }
            }
        }
    }

    if hash.is_empty() {
        println!("cargo:rustc-env=GIT_HASH=unknown");
    } else {
        println!("cargo:rustc-env=GIT_HASH={hash}");
    }
}
