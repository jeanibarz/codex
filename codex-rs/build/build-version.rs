use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let repo_root = PathBuf::from(manifest_dir)
        .parent()
        .expect("crate dir should live under codex-rs")
        .parent()
        .expect("codex-rs should live under repo root")
        .to_path_buf();

    register_rerun_inputs(&repo_root);

    let base_version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let git_sha = git_stdout(&repo_root, &["rev-parse", "--short=9", "HEAD"])
        .unwrap_or_else(|| "unknown".to_string());
    let is_dirty = Command::new("git")
        .args(["diff-index", "--quiet", "HEAD", "--"])
        .current_dir(&repo_root)
        .status()
        .map(|status| !status.success())
        .unwrap_or(false);

    let dirty_suffix = if is_dirty { ".dirty" } else { "" };
    let build_version = format!("{base_version}+looper.{git_sha}{dirty_suffix}");
    println!("cargo:rustc-env=CODEX_BUILD_VERSION={build_version}");
}

fn register_rerun_inputs(repo_root: &PathBuf) {
    let git_dir = repo_root.join(".git");
    println!("cargo:rerun-if-changed={}", git_dir.join("HEAD").display());
    println!("cargo:rerun-if-changed={}", git_dir.join("index").display());

    if let Ok(head_ref) = std::fs::read_to_string(git_dir.join("HEAD"))
        && let Some(ref_path) = head_ref.strip_prefix("ref: ")
    {
        println!(
            "cargo:rerun-if-changed={}",
            git_dir.join(ref_path.trim()).display()
        );
    }
}

fn git_stdout(repo_root: &PathBuf, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
