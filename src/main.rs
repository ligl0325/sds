use sds::index::SdsIndex;
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

fn main() -> anyhow::Result<()> {
    use clap::Parser;
    let cli = Cli::parse();

    let sds_dir = data_dir()?;
    ensure_dir(&sds_dir)?;

    // 文件锁
    let lock_path = sds_dir.join("sds.lock");
    let lock_file = std::fs::File::create(&lock_path)?;
    fs2::FileExt::try_lock_exclusive(&lock_file)
        .map_err(|_| anyhow::anyhow!("无法获取文件锁，可能有其他进程正在使用"))?;

    // 一次性打开索引，所有命令复用
    let mut index = SdsIndex::open(&sds_dir)?;

    match &cli.command {
        Command::Store { text, source, tags } => cmd_store(&mut index, text, source, tags),
        Command::Search {
            query,
            top,
            json,
            tag,
            source,
        } => cmd_search(
            &index,
            query,
            *top,
            *json,
            tag.as_deref(),
            source.as_deref(),
        ),
        Command::List {
            limit,
            offset,
            json,
            source,
        } => cmd_list(&index, *limit, *offset, *json, source.as_deref()),
        Command::Delete { id, source } => cmd_delete(&mut index, *id, source.as_deref()),
        Command::Status { json } => cmd_status(&index, *json),
        Command::Export { format } => cmd_export(&index, format),
        Command::Compact => cmd_compact(&mut index),
        Command::Import { dir, source, tags } => cmd_import(&mut index, dir, source, tags),
        Command::Migrate { path } => cmd_migrate_sqlite(&mut index, path),
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

fn cmd_store(index: &mut SdsIndex, text: &str, source: &str, tags: &str) -> anyhow::Result<()> {
    let mem = index.store(text, source, tags)?;
    println!("✅ 已存储 (id={})  |  {}", mem.id, fmt_ts(mem.created_at));
    println!("   text:   {}", mem.text);
    println!("   source: {}", mem.source);
    println!("   tags:   {}", mem.tags);
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

fn cmd_delete(index: &mut SdsIndex, id: Option<u64>, source: Option<&str>) -> anyhow::Result<()> {
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
    let status = index.status()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else {
        println!("📊 闪搜 SDS 状态\n{}", "=".repeat(30));
        println!("  记忆: {} 条", status.memories);
        println!("  索引: {}", status.index_size);
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
            println!("id,text,source,tags,created_at");
            for mem in &results {
                let text_escaped = mem.text.replace('"', "\"\"");
                let source_escaped = mem.source.replace('"', "\"\"");
                let tags_escaped = mem.tags.replace('"', "\"\"");
                println!(
                    "{},{},{},{},{}",
                    mem.id, text_escaped, source_escaped, tags_escaped, mem.created_at
                );
            }
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

fn cmd_compact(index: &mut SdsIndex) -> anyhow::Result<()> {
    index.compact()?;
    println!("✅ compact 完成");
    Ok(())
}

/// 导入目录下的所有 .md 文件
fn cmd_import(index: &mut SdsIndex, dir: &str, source: &str, tags: &str) -> anyhow::Result<()> {
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

    index.writer().commit()?;
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

fn cmd_migrate_sqlite(index: &mut SdsIndex, path: &str) -> anyhow::Result<()> {
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
        index.writer().commit()?;
        let counter_path = index.data_dir().join("counter");
        std::fs::write(&counter_path, max_id.to_string())?;
    }
    println!("✅ 迁移完成: {} 条记忆 (max_id={})", count, max_id);
    Ok(())
}
