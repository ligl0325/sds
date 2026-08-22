use sds::index::{SdsIndex, SdsWriter};
use std::path::PathBuf;

/// 闪搜 SDS — Smart Data Search: 轻量中文记忆 CLI 工具
#[derive(clap::Parser)]
#[command(name = "sds", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    Store {
        text: String,
        #[arg(long, default_value = "cli")]
        source: String,
        #[arg(long, default_value = "")]
        tags: String,
        #[arg(long = "type", default_value = "fact")]
        memory_type: String,
        #[arg(long, default_value_t = 50.0)]
        importance: f64,
        #[arg(long)]
        dedupe: bool,
        #[arg(long)]
        upsert_id: Option<u64>,
    },
    Search {
        query: String,
        #[arg(long, default_value_t = 10)]
        top: usize,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        source: Option<String>,
    },
    List {
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        source: Option<String>,
    },
    Delete {
        id: Option<u64>,
        #[arg(long)]
        source: Option<String>,
    },
    Status {
        #[arg(long)]
        json: bool,
    },
    Export {
        #[arg(long, default_value = "json")]
        format: String,
    },
    Compact,
    /// 导入目录下的所有 .md 文件到 sds
    Import {
        /// 要导入的目录路径
        dir: String,
        /// 来源标签
        #[arg(long, default_value = "novel/星火燎原")]
        source: String,
        /// 标签（逗号分隔）
        #[arg(long, default_value = "小说,星火燎原")]
        tags: String,
    },
    /// 从兼容的 SQLite 数据库导入记忆
    Migrate {
        /// SQLite 数据库路径
        path: String,
    },
    /// 备份整个SDS数据目录
    Backup {
        /// 备份目标目录（必须不存在）
        destination: PathBuf,
    },
    /// 从备份目录恢复SDS数据
    Restore {
        /// 备份目录路径
        source: PathBuf,
        /// 恢复前后都执行完整索引校验
        #[arg(long)]
        verify: bool,
    },
    /// 执行标准检索基准
    Benchmark {
        /// 基准查询
        #[arg(long, default_value = "Hermes")]
        query: String,
        /// 重复次数
        #[arg(long, default_value_t = 20)]
        repeat: usize,
        /// 每次返回条数
        #[arg(long, default_value_t = 10)]
        top: usize,
        /// JSON输出
        #[arg(long)]
        json: bool,
    },
}

fn data_dir() -> anyhow::Result<PathBuf> {
    let dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("无法获取 home 目录"))?
        .join(".sds");
    Ok(dir)
}

fn ensure_dir(dir: &PathBuf) -> anyhow::Result<()> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn copy_tree(source: &std::path::Path, destination: &std::path::Path) -> anyhow::Result<()> {
    if destination.exists() {
        anyhow::bail!(
            "目标目录已存在，为避免覆盖请换一个路径: {}",
            destination.display()
        );
    }
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == "sds.lock" || name == ".counter.tmp" || name == "schema_version.tmp" {
            continue;
        }
        let source_path = entry.path();
        let destination_path = destination.join(name);
        if source_path.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else {
            std::fs::copy(source_path, destination_path)?;
        }
    }
    Ok(())
}

fn verify_backup(path: &std::path::Path) -> anyhow::Result<serde_json::Value> {
    if !path.join("tantivy_index/meta.json").exists() {
        anyhow::bail!("不是有效的SDS备份目录: {}", path.display());
    }
    let index = SdsIndex::open_readonly(path)?;
    let status = index.status();
    Ok(serde_json::json!({
        "memories": status.memories,
        "segments": status.segments,
        "files": status.files,
        "index_size": status.index_size,
        "schema_version": status.schema_version,
    }))
}

fn cmd_backup(index: &mut SdsWriter, destination: &std::path::Path) -> anyhow::Result<()> {
    if destination.starts_with(index.data_dir()) {
        anyhow::bail!("备份目标不能位于当前数据目录内部");
    }
    index.commit()?;
    copy_tree(index.data_dir(), destination)?;
    let report = verify_backup(destination)?;
    println!("✅ 备份完成: {}", destination.display());
    println!("   校验: {}", serde_json::to_string(&report)?);
    Ok(())
}

fn acquire_data_lock(data_dir: &std::path::Path) -> anyhow::Result<std::fs::File> {
    std::fs::create_dir_all(data_dir)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(data_dir.join("sds.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .map_err(|_| anyhow::anyhow!("无法获取恢复锁，可能有其他写入进程正在使用"))?;
    Ok(lock)
}

fn cmd_restore(
    data_dir: &std::path::Path,
    source: &std::path::Path,
    verify: bool,
) -> anyhow::Result<()> {
    let source = source.canonicalize()?;
    let data_dir = data_dir
        .canonicalize()
        .unwrap_or_else(|_| data_dir.to_path_buf());
    if source.starts_with(&data_dir) {
        anyhow::bail!("备份源不能位于当前数据目录内部");
    }
    let before = verify_backup(&source)?;
    if verify {
        println!("📋 恢复前校验: {}", serde_json::to_string(&before)?);
    }

    let _lock = acquire_data_lock(&data_dir)?;
    let staging = data_dir.with_file_name(".sds.restore-staging");
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    copy_tree(&source, &staging)?;
    let staged_report = verify_backup(&staging)?;

    let backup_dir = data_dir.with_file_name(format!(
        ".sds.pre-restore-{}",
        chrono::Utc::now().format("%Y%m%d_%H%M%S")
    ));
    std::fs::rename(&data_dir, &backup_dir)?;
    std::fs::rename(&staging, &data_dir)?;

    let after = verify_backup(&data_dir)?;
    if verify && staged_report != after {
        anyhow::bail!("恢复后校验结果与暂存备份不一致");
    }
    println!("✅ 恢复完成: {}", data_dir.display());
    println!("   恢复前: {}", serde_json::to_string(&before)?);
    println!("   恢复后: {}", serde_json::to_string(&after)?);
    println!("   原数据保留: {}", backup_dir.display());
    Ok(())
}

#[derive(serde::Serialize)]
struct BenchmarkReport {
    query: String,
    repeat: usize,
    top: usize,
    result_count: usize,
    min_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    max_ms: f64,
    avg_ms: f64,
    rss_kb: Option<u64>,
}

fn current_rss_kb() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/self/status").ok()?;
    text.lines()
        .find(|line| line.starts_with("VmRSS:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse().ok())
}

fn cmd_benchmark(
    index: &SdsIndex,
    query: &str,
    repeat: usize,
    top: usize,
    json: bool,
) -> anyhow::Result<()> {
    let repeat = repeat.max(1);
    let _ = index.search(query, top, None, None)?;
    let mut samples = Vec::with_capacity(repeat);
    let mut result_count = 0;
    for _ in 0..repeat {
        let started = std::time::Instant::now();
        result_count = index.search(query, top, None, None)?.len();
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    let p50 = samples[(samples.len() - 1) * 50 / 100];
    let p95 = samples[(samples.len() - 1) * 95 / 100];
    let report = BenchmarkReport {
        query: query.to_string(),
        repeat,
        top,
        result_count,
        min_ms: samples[0],
        p50_ms: p50,
        p95_ms: p95,
        max_ms: *samples.last().unwrap_or(&0.0),
        avg_ms: samples.iter().sum::<f64>() / samples.len() as f64,
        rss_kb: current_rss_kb(),
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("📈 SDS Benchmark");
        println!("  query: {}", report.query);
        println!(
            "  repeat: {}  results: {}",
            report.repeat, report.result_count
        );
        println!("  min: {:.3}ms", report.min_ms);
        println!("  p50: {:.3}ms", report.p50_ms);
        println!("  p95: {:.3}ms", report.p95_ms);
        println!("  max: {:.3}ms", report.max_ms);
        println!("  avg: {:.3}ms", report.avg_ms);
        if let Some(rss) = report.rss_kb {
            println!("  RSS: {}KB", rss);
        }
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    use clap::Parser;
    let cli = Cli::parse();

    let sds_dir = data_dir()?;
    ensure_dir(&sds_dir)?;

    match &cli.command {
        Command::Store {
            text,
            source,
            tags,
            memory_type,
            importance,
            dedupe,
            upsert_id,
        } => {
            let mut writer = SdsWriter::open(&sds_dir)?;
            cmd_store(
                &mut writer,
                text,
                source,
                tags,
                StoreOptions {
                    memory_type,
                    importance: *importance,
                    dedupe: *dedupe,
                    upsert_id: *upsert_id,
                },
            )
        }
        Command::Search {
            query,
            top,
            json,
            tag,
            source,
        } => {
            let index = SdsIndex::open_readonly(&sds_dir)?;
            cmd_search(
                &index,
                query,
                *top,
                *json,
                tag.as_deref(),
                source.as_deref(),
            )
        }
        Command::List {
            limit,
            offset,
            json,
            source,
        } => {
            let index = SdsIndex::open_readonly(&sds_dir)?;
            cmd_list(&index, *limit, *offset, *json, source.as_deref())
        }
        Command::Delete { id, source } => {
            let mut writer = SdsWriter::open(&sds_dir)?;
            cmd_delete(&mut writer, *id, source.as_deref())
        }
        Command::Status { json } => {
            let index = SdsIndex::open_readonly(&sds_dir)?;
            cmd_status(&index, *json)
        }
        Command::Export { format } => {
            let index = SdsIndex::open_readonly(&sds_dir)?;
            cmd_export(&index, format)
        }
        Command::Compact => {
            let mut writer = SdsWriter::open(&sds_dir)?;
            cmd_compact(&mut writer)
        }
        Command::Import { dir, source, tags } => {
            let mut writer = SdsWriter::open(&sds_dir)?;
            cmd_import(&mut writer, dir, source, tags)
        }
        Command::Migrate { path } => {
            let mut writer = SdsWriter::open(&sds_dir)?;
            cmd_migrate_sqlite(&mut writer, path)
        }
        Command::Backup { destination } => {
            let mut writer = SdsWriter::open(&sds_dir)?;
            cmd_backup(&mut writer, destination)
        }
        Command::Restore { source, verify } => cmd_restore(&sds_dir, source, *verify),
        Command::Benchmark {
            query,
            repeat,
            top,
            json,
        } => {
            let index = SdsIndex::open_readonly(&sds_dir)?;
            cmd_benchmark(&index, query, *repeat, *top, *json)
        }
    }
}

fn fmt_ts(ts: f64) -> String {
    chrono::DateTime::from_timestamp(ts as i64, 0)
        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

fn truncate(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() > max_chars {
        let s: String = chars[..max_chars].iter().collect();
        format!("{}...", s)
    } else {
        text.to_string()
    }
}

// ── 命令实现 ──

struct StoreOptions<'a> {
    memory_type: &'a str,
    importance: f64,
    dedupe: bool,
    upsert_id: Option<u64>,
}

fn cmd_store(
    index: &mut SdsWriter,
    text: &str,
    source: &str,
    tags: &str,
    options: StoreOptions<'_>,
) -> anyhow::Result<()> {
    let mem = if let Some(id) = options.upsert_id {
        index.replace(
            id,
            text,
            source,
            tags,
            options.memory_type,
            options.importance,
        )?
    } else {
        index.store_with_options(
            text,
            source,
            tags,
            options.memory_type,
            options.importance,
            options.dedupe,
        )?
    };
    println!("✅ 已存储 (id={})  |  {}", mem.id, fmt_ts(mem.created_at));
    println!("   text:   {}", mem.text);
    println!("   source: {}", mem.source);
    println!("   tags:   {}", mem.tags);
    println!("   type:   {}", mem.memory_type);
    println!("   score:  {:.1}", mem.importance);
    Ok(())
}

fn cmd_search(
    index: &SdsIndex,
    query: &str,
    top: usize,
    json: bool,
    tag: Option<&str>,
    source: Option<&str>,
) -> anyhow::Result<()> {
    if query.trim().is_empty() {
        return Err(anyhow::anyhow!("查询不能为空"));
    }
    let results = index.search(query, top, tag, source)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else if results.is_empty() {
        println!("🔍 {}\n   无匹配结果", query);
    } else {
        let mut filter_info = String::new();
        if let Some(t) = tag {
            filter_info += &format!(" | tag={}", t);
        }
        if let Some(s) = source {
            filter_info += &format!(" | source={}", s);
        }
        println!("🔍 {}{}\n{}", query, filter_info, "=".repeat(60));
        for mem in &results {
            let score = mem.score.unwrap_or(0.0);
            let preview = truncate(&mem.text, 150);
            println!(
                "\n  [{:4}]  得分: {:.2}  |  {}  |  source={}  |  tags={}",
                mem.id,
                score,
                fmt_ts(mem.created_at),
                mem.source,
                mem.tags
            );
            println!("         {}", preview);
        }
    }
    Ok(())
}

fn cmd_list(
    index: &SdsIndex,
    limit: usize,
    offset: usize,
    json: bool,
    source: Option<&str>,
) -> anyhow::Result<()> {
    let results = index.list(limit, offset, source)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else if results.is_empty() {
        println!("📭 暂无记忆");
    } else {
        let filter_info = source
            .map(|s| format!(" | source={}", s))
            .unwrap_or_default();
        println!(
            "📋 最近 {} 条记忆 (offset={}){}",
            limit, offset, filter_info
        );
        println!("{}", "=".repeat(60));
        for mem in &results {
            let preview = truncate(&mem.text, 100);
            println!(
                "  [{:4}] {}  source={}  tags={}  {}",
                mem.id,
                fmt_ts(mem.created_at),
                mem.source,
                mem.tags,
                preview
            );
        }
    }
    Ok(())
}

fn cmd_delete(index: &mut SdsWriter, id: Option<u64>, source: Option<&str>) -> anyhow::Result<()> {
    match (id, source) {
        (Some(id), None) => {
            if index.delete(id)? {
                println!("✅ 已删除 id={}", id);
            } else {
                println!("❌ 未找到 id={}", id);
            }
        }
        (None, Some(src)) => {
            let count = index.delete_by_source(src)?;
            if count > 0 {
                println!("✅ 已删除 {} 条 source={} 的记录", count, src);
            } else {
                println!("❌ 未找到 source={} 的记录", src);
            }
        }
        (Some(_), Some(_)) => {
            println!("❌ 不能同时指定 id 和 --source");
        }
        (None, None) => {
            println!("❌ 请指定要删除的 id 或 --source");
        }
    }
    Ok(())
}

fn cmd_status(index: &SdsIndex, json: bool) -> anyhow::Result<()> {
    let status = index.status();
    if json {
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else {
        println!("📊 闪搜 SDS 状态\n{}", "=".repeat(30));
        println!("  记忆: {} 条", status.memories);
        println!("  索引: {}", status.index_size);
        println!("  Segment: {}", status.segments);
        println!("  文件: {}", status.files);
        println!("  碎片率: {:.4}%", status.fragmentation_rate);
        println!("  Schema: v{}", status.schema_version);
        println!("  路径: {}", status.index_path);
        println!("  引擎: Tantivy + jieba 中文分词 (无需 GPU)");
    }
    Ok(())
}

fn cmd_export(index: &SdsIndex, format: &str) -> anyhow::Result<()> {
    let results = index.all()?;
    match format {
        "json" => println!("{}", serde_json::to_string_pretty(&results)?),
        "csv" => {
            let mut writer = csv::Writer::from_writer(std::io::stdout());
            writer.write_record(["id", "text", "source", "tags", "created_at"])?;
            for memory in &results {
                writer.serialize((
                    memory.id,
                    &memory.text,
                    &memory.source,
                    &memory.tags,
                    memory.created_at,
                ))?;
            }
            writer.flush()?;
        }
        _ => {
            return Err(anyhow::anyhow!(
                "不支持的格式: {}（支持 json / csv）",
                format
            ));
        }
    }
    Ok(())
}

fn cmd_compact(index: &mut SdsWriter) -> anyhow::Result<()> {
    let stats = index.compact()?;
    let segment_pct = stats
        .segments_before
        .saturating_sub(stats.segments_after)
        .saturating_mul(100)
        .checked_div(stats.segments_before)
        .unwrap_or(0);
    let file_pct = stats
        .files_before
        .saturating_sub(stats.files_after)
        .saturating_mul(100)
        .checked_div(stats.files_before)
        .unwrap_or(0);
    println!("✅ 索引合并完成");
    println!(
        "   Segment:  {} → {}  (-{}%)",
        stats.segments_before, stats.segments_after, segment_pct
    );
    println!(
        "   文件:     {} → {}  (-{}%)",
        stats.files_before, stats.files_after, file_pct
    );
    println!("   索引:     {} → {}", stats.size_before, stats.size_after);
    println!("   文档:     {}", stats.memories);
    println!("   合并批次: {}", stats.merge_operations);
    println!("   耗时:     {:.2}s", stats.elapsed_ms as f64 / 1000.0);
    Ok(())
}

/// 导入目录下的所有 .md 文件
fn cmd_import(index: &mut SdsWriter, dir: &str, source: &str, tags: &str) -> anyhow::Result<()> {
    let base = dir.replace("~", &dirs::home_dir().unwrap().to_string_lossy());
    let base_path = std::path::Path::new(&base);
    if !base_path.exists() {
        return Err(anyhow::anyhow!("目录不存在: {}", base));
    }

    let mut entries: Vec<_> = Vec::new();
    collect_md_files(base_path, &mut entries)?;
    let total = entries.len();
    if total == 0 {
        eprintln!("⚠️ 目录中无 .md 文件: {}", base);
        return Ok(());
    }

    eprintln!("📋 找到 {} 个 .md 文件，开始导入...", total);
    let mut count = 0u64;
    for entry in &entries {
        let content = std::fs::read_to_string(entry)?;
        // 取文件名（不含扩展名）作为额外 tag
        let file_tag = entry.file_stem().unwrap().to_string_lossy();
        let full_tags = format!("{},{}", tags, file_tag);

        index.batch_store(&content, source, &full_tags)?;
        count += 1;
        if count.is_multiple_of(100) {
            eprint!("  已导入: {} / {} ...\r", count, total);
        }
    }

    index.commit()?;
    eprintln!("  已导入: {} / {} ", count, total);
    println!(
        "✅ 导入完成: {} 条记忆（source={}, tags={}）",
        count, source, tags
    );
    Ok(())
}

fn collect_md_files(
    dir: &std::path::Path,
    files: &mut Vec<std::path::PathBuf>,
) -> std::io::Result<()> {
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                collect_md_files(&path, files)?;
            } else if let Some(ext) = path.extension() {
                let ext = ext.to_string_lossy().to_lowercase();
                if ext == "md" || ext == "txt" {
                    files.push(path);
                }
            }
        }
    }
    Ok(())
}

// ── migrate 命令 ──

fn auto_map_tags(source: &str) -> String {
    if source.starts_with("github") {
        let parts: Vec<&str> = source.split('/').collect();
        if parts.len() >= 2 {
            return format!("github,{},调研", parts[1]);
        }
        return "github,调研".to_string();
    }
    if source == "cli" {
        return "cli,杂项".to_string();
    }
    let prefix = source.split('/').next().unwrap_or(source);
    format!("{},misc", prefix)
}

fn cmd_migrate_sqlite(index: &mut SdsWriter, path: &str) -> anyhow::Result<()> {
    let py_path = path.replace("~", &dirs::home_dir().unwrap().to_string_lossy());
    if !std::path::Path::new(&py_path).exists() {
        eprint!("Python 版数据库不存在: {}", py_path);
        eprint!("跳过迁移");
        return Ok(());
    }
    let conn = rusqlite::Connection::open(&py_path)?;
    let mut stmt = conn.prepare("SELECT id, text, source, created_at FROM memories ORDER BY id")?;
    let existing_max = index.max_id()?;
    let mut count = 0u64;
    let mut max_id = existing_max;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, u64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, f64>(3)?,
        ))
    })?;
    for row in rows {
        let (id, text, source, _created_at) = row?;
        if id <= existing_max {
            continue;
        }
        index.batch_store(&text, &source, &auto_map_tags(&source))?;
        if id > max_id {
            max_id = id;
        }
        count += 1;
    }
    if count > 0 {
        index.commit()?;
        index.set_counter(max_id)?;
    }
    println!("✅ 迁移完成: {} 条记忆 (max_id={})", count, max_id);
    Ok(())
}
