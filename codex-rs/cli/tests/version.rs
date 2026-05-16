use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn reports_build_version_with_kookr_git_metadata() {
    let mut command =
        Command::new(codex_utils_cargo_bin::cargo_bin("codex").expect("codex binary"));
    command
        .arg("--version")
        .assert()
        .success()
        .stdout(contains("+kookr."));
}
