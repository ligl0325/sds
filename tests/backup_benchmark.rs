use std::process::Command;

use tempfile::TempDir;

fn sds_bin() -> &'static str {
    env!("CARGO_BIN_EXE_sds")
}

#[test]
fn test_it_backup_001_backup_restore_verify_roundtrip() -> anyhow::Result<()> {
    let root = TempDir::new()?;
    let home = root.path().join("home");
    let backup = root.path().join("backup");
    std::fs::create_dir_all(&home)?;

    let run = |args: &[&str]| -> anyhow::Result<std::process::Output> {
        Ok(Command::new(sds_bin())
            .args(args)
            .env("HOME", &home)
            .output()?)
    };

    for i in 0..3 {
        let output = run(&[
            "store",
            &format!("备份恢复文档{i}"),
            "--source",
            "test",
            "--tags",
            "backup",
        ])?;
        assert!(output.status.success());
    }

    let backup_arg = backup.to_string_lossy().to_string();
    let output = run(&["backup", &backup_arg])?;
    assert!(output.status.success());
    assert!(backup.join("tantivy_index/meta.json").exists());

    let output = run(&["store", "不应保留的记录"])?;
    assert!(output.status.success());
    let output = run(&["restore", &backup_arg, "--verify"])?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let output = run(&["status", "--json"])?;
    let status: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(status["memories"], 3);
    assert_eq!(status["schema_version"], 1);
    let old_dirs = std::fs::read_dir(&home)?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".sds.pre-restore-")
        })
        .count();
    assert_eq!(old_dirs, 1);
    Ok(())
}

#[test]
fn test_it_benchmark_002_json_report_shape() -> anyhow::Result<()> {
    let home = TempDir::new()?;
    let output = Command::new(sds_bin())
        .args(["benchmark", "--query", "smoke", "--repeat", "3", "--json"])
        .env("HOME", home.path())
        .output()?;
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(report["repeat"], 3);
    assert!(report["p50_ms"].as_f64().unwrap() >= 0.0);
    assert!(report["p95_ms"].as_f64().unwrap() >= report["p50_ms"].as_f64().unwrap());
    Ok(())
}
