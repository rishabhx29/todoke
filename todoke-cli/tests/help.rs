//! Workspace smoke test: proves the todoke binary builds, runs, and prints
//! its help screen on every CI platform.

use assert_cmd::Command;

#[test]
fn help_screen_lists_all_subcommands() {
    let output = Command::cargo_bin("todoke")
        .expect("todoke binary should be built")
        .arg("--help")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout).into_owned();
    for sub in ["serve", "send", "devices", "bench"] {
        assert!(
            stdout.contains(sub),
            "help should mention `{sub}`:\n{stdout}"
        );
    }
}

#[test]
fn version_flag_prints_engine_version() {
    let output = Command::cargo_bin("todoke")
        .expect("todoke binary should be built")
        .arg("--version")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout).into_owned();
    assert!(
        stdout.contains(todoke_core::VERSION),
        "version output should carry the engine version {v}:\n{stdout}",
        v = todoke_core::VERSION
    );
}
