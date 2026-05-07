use std::process::Command;

use tempfile::tempdir;

#[test]
fn insights_since_on_missing_root_prints_empty_report_without_creating_files() {
    let td = tempdir().unwrap();
    let root = td.path().join("missing-root");

    let output = Command::new(env!("CARGO_BIN_EXE_qmonster"))
        .args(["--root", root.to_str().unwrap(), "insights", "--since", "24h"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Token Insights"));
    assert!(stdout.contains("Situations\n  none"));
    assert!(stdout.contains("Action Ledger\n  none"));
    assert!(stdout.contains("Recent Timeline\n  none"));
    assert!(!root.exists());
}

#[test]
fn once_with_subcommand_is_rejected_before_work() {
    let td = tempdir().unwrap();
    let root = td.path().join("missing-root");

    let output = Command::new(env!("CARGO_BIN_EXE_qmonster"))
        .args([
            "--once",
            "--root",
            root.to_str().unwrap(),
            "insights",
            "--since",
            "24h",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("--once cannot be combined with a subcommand"));
    assert!(!root.exists());
}
