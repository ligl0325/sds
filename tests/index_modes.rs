use sds::{SdsIndex, SdsWriter};
use tempfile::TempDir;

#[test]
fn test_it_index_001_reader_coexists_with_active_writer() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let data_dir = temp.path().join(".sds");

    let mut writer = SdsWriter::open(&data_dir)?;
    writer.store("读写并存测试", "test", "reader,writer")?;

    let reader = SdsIndex::open_readonly(&data_dir)?;
    let results = reader.search("读写并存", 10, None, None)?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].source, "test");
    Ok(())
}

#[test]
fn test_it_index_002_second_writer_is_rejected() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let data_dir = temp.path().join(".sds");

    let _first_writer = SdsWriter::open(&data_dir)?;
    let second_writer = SdsWriter::open(&data_dir);
    assert!(second_writer.is_err(), "第二个写入器必须被独占锁拒绝");
    Ok(())
}

#[test]
fn test_it_index_003_readonly_handle_does_not_block_writer() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let data_dir = temp.path().join(".sds");

    let reader = SdsIndex::open_readonly(&data_dir)?;
    assert_eq!(reader.status().memories, 0);

    let mut writer = SdsWriter::open(&data_dir)?;
    writer.store("只读句柄不阻塞写入", "test", "lock")?;
    assert_eq!(writer.status().memories, 1);
    Ok(())
}

#[test]
fn test_it_index_004_store_delete_roundtrip() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let data_dir = temp.path().join(".sds");

    let mut writer = SdsWriter::open(&data_dir)?;
    let memory = writer.store("CRUD回归测试", "test", "crud")?;
    assert_eq!(writer.search("CRUD回归", 10, None, None)?.len(), 1);

    assert!(writer.delete(memory.id)?);
    assert!(writer.search("CRUD回归", 10, None, None)?.is_empty());
    Ok(())
}
