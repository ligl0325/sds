use std::process::Command;

use tempfile::TempDir;

fn sds_bin() -> &'static str {
    env!("CARGO_BIN_EXE_sds")
}

#[test]
fn test_it_export_001_csv_roundtrips_special_characters() -> anyhow::Result<()> {
    let home = TempDir::new()?;
    let text = "CSV,引号\"和真实换行\n第二行";
    let source = "source,with,comma";
    let tags = "csv,\"quoted\"";

    let store = Command::new(sds_bin())
        .args(["store", text, "--source", source, "--tags", tags])
        .env("HOME", home.path())
        .output()?;
    assert!(store.status.success());

    let export = Command::new(sds_bin())
        .args(["export", "--format", "csv"])
        .env("HOME", home.path())
        .output()?;
    assert!(
        export.status.success(),
        "CSV导出失败: {}",
        String::from_utf8_lossy(&export.stderr)
    );

    let mut reader = csv::Reader::from_reader(export.stdout.as_slice());
    assert_eq!(
        reader.headers()?.iter().collect::<Vec<_>>(),
        vec!["id", "text", "source", "tags", "created_at"]
    );
    let record = reader.records().next().expect("应有一条CSV记录")?;
    assert_eq!(&record[1], text);
    assert_eq!(&record[2], source);
    assert_eq!(&record[3], tags);
    assert!(reader.records().next().is_none());
    Ok(())
}
