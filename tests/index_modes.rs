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

#[test]
fn test_it_index_005_compact_merges_segments_and_preserves_data() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let data_dir = temp.path().join(".sds");

    // 模拟CLI逐条调用：每次启动一个Writer并提交一条，制造单文档段。
    for index in 0..20 {
        let mut writer = SdsWriter::open(&data_dir)?;
        writer.store(&format!("合并回归文档 {index}"), "compact-test", "compact")?;
    }

    let mut writer = SdsWriter::open(&data_dir)?;
    let segments_before = writer.segment_ids().len();
    assert!(segments_before > 1, "测试必须先制造多个Segment");

    let stats = writer.compact()?;
    assert_eq!(stats.segments_before, segments_before);
    assert_eq!(stats.segments_after, 1);
    assert_eq!(stats.memories, 20);
    assert!(stats.merge_operations >= 1);
    assert!(stats.files_after < stats.files_before);
    assert_eq!(writer.search("合并回归", 30, None, None)?.len(), 20);

    let second = writer.compact()?;
    assert_eq!(second.segments_before, 1);
    assert_eq!(second.segments_after, 1);
    assert_eq!(second.memories, 20);
    assert_eq!(second.merge_operations, 0);
    Ok(())
}

#[test]
fn test_it_index_006_store_auto_limits_segment_growth() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let data_dir = temp.path().join(".sds");

    for index in 0..40 {
        let mut writer = SdsWriter::open(&data_dir)?;
        writer.store(
            &format!("自动归并文档 {index}"),
            "auto-compact-test",
            "compact",
        )?;
    }

    let reader = SdsIndex::open_readonly(&data_dir)?;
    let segment_count = reader.segment_ids().len();
    assert!(segment_count <= 32, "段数必须受自动归并阈值约束");
    assert!(segment_count < 40, "自动归并必须实际发生");
    assert_eq!(reader.status().memories, 40);
    assert_eq!(reader.search("自动归并", 50, None, None)?.len(), 40);
    Ok(())
}

#[test]
fn test_it_index_007_malformed_query_falls_back_to_literal() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let data_dir = temp.path().join(".sds");
    let mut writer = SdsWriter::open(&data_dir)?;
    writer.store("函数(foo:bar) 路径 C:\\tmp", "code:test", "query")?;

    assert!(!writer.search("函数(foo:bar)", 10, None, None)?.is_empty());
    assert!(writer.search("(", 10, None, None)?.is_empty());
    assert!(writer.search("\"未闭合", 10, None, None).is_ok());
    assert!(writer.search("路径 C:\\tmp", 10, None, None).is_ok());
    assert!(
        writer
            .search("函数", 10, Some("query("), Some("code:test"))
            .is_ok()
    );
    Ok(())
}

#[test]
fn test_it_index_008_counter_update_is_atomic_and_continuous() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let data_dir = temp.path().join(".sds");
    let mut writer = SdsWriter::open(&data_dir)?;

    writer.set_counter(41)?;
    assert_eq!(writer.max_id()?, 41);
    assert!(!data_dir.join(".counter.tmp").exists());

    let memory = writer.store("原子计数器测试", "test", "counter")?;
    assert_eq!(memory.id, 42);
    assert_eq!(writer.max_id()?, 42);
    assert!(!data_dir.join(".counter.tmp").exists());
    Ok(())
}

#[test]
fn test_it_index_009_future_schema_is_rejected() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let data_dir = temp.path().join(".sds");
    let _writer = SdsWriter::open(&data_dir)?;
    std::fs::write(data_dir.join("schema_version"), "2")?;

    let result = SdsIndex::open_readonly(&data_dir);
    assert!(result.is_err(), "当前程序不能静默读取未来Schema");
    Ok(())
}
