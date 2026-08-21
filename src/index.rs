use std::fs::{File, OpenOptions};
use std::io::Write;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use cang_jie::CangJieTokenizer;
use fs2::FileExt;
use tantivy::index::SegmentId;
use tantivy::indexer::NoMergePolicy;
use tantivy::query::{AllQuery, BooleanQuery, EmptyQuery, Occur, QueryParser, TermQuery};
use tantivy::schema::*;
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument};

const COUNTER_FILE: &str = "counter";
const INDEX_DIR: &str = "tantivy_index";
const LOCK_FILE: &str = "sds.lock";
const WRITER_MEMORY_BUDGET: usize = 50_000_000;
const COMPACT_MERGE_BATCH_SIZE: usize = 64;
const AUTO_COMPACT_SEGMENT_THRESHOLD: usize = 32;

#[derive(serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct Memory {
    pub id: u64,
    pub text: String,
    pub source: String,
    pub tags: String,
    pub created_at: f64,
    pub score: Option<f64>,
}

/// 只读索引句柄：不创建 IndexWriter，也不获取独占锁。
pub struct SdsIndex {
    index: Index,
    fields: SdsFields,
    reader: IndexReader,
    data_dir: PathBuf,
}

/// 写入索引句柄：持有进程级文件锁和 Tantivy IndexWriter。
pub struct SdsWriter {
    index: SdsIndex,
    writer: IndexWriter,
    _lock_file: File,
}

struct SdsFields {
    id: Field,
    text: Field,
    text_lower: Field,
    source: Field,
    tags: Field,
    created_at: Field,
}

fn jieba_text_option() -> TextOptions {
    TextOptions::default().set_stored().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer("cang_jie")
            .set_index_option(IndexRecordOption::WithFreqsAndPositions)
            .set_fieldnorms(true),
    )
}

fn build_schema() -> Schema {
    let mut builder = Schema::builder();
    builder.add_u64_field("id", FAST | STORED);
    builder.add_text_field("text", STRING | STORED);
    builder.add_text_field("text_lower", jieba_text_option());
    builder.add_text_field("source", jieba_text_option());
    builder.add_text_field("tags", jieba_text_option());
    builder.add_f64_field("created_at", FAST | STORED);
    builder.build()
}

fn parse_query_with_literal_fallback(
    parser: &QueryParser,
    input: &str,
) -> anyhow::Result<Box<dyn tantivy::query::Query>> {
    if !input.chars().any(char::is_alphanumeric) {
        return Ok(Box::new(EmptyQuery));
    }

    if let Ok(query) = parser.parse_query(input) {
        return Ok(query);
    }

    // 保留正常AND/OR/短语语法；只有语法错误时才退化为安全字面量短语。
    let escaped = input.replace('\\', "\\\\").replace('"', "\\\"");
    let quoted = format!("\"{escaped}\"");
    if let Ok(query) = parser.parse_query(&quoted) {
        return Ok(query);
    }

    // 纯标点可能无法产生词元；lenient解析保证查询请求不因用户文本崩溃。
    Ok(parser.parse_query_lenient(input).0)
}

fn open_or_create_index(data_dir: &Path) -> anyhow::Result<Index> {
    let index_dir = data_dir.join(INDEX_DIR);
    std::fs::create_dir_all(&index_dir)?;

    let index = if index_dir.join("meta.json").exists() {
        Index::open_in_dir(&index_dir)?
    } else {
        match Index::create_in_dir(&index_dir, build_schema()) {
            Ok(index) => index,
            // 多个首次只读进程可能同时判断 meta.json 不存在；输掉创建竞态的进程改为打开。
            Err(create_error) => Index::open_in_dir(&index_dir).map_err(|open_error| {
                anyhow::anyhow!("创建索引失败: {create_error}; 随后打开索引也失败: {open_error}")
            })?,
        }
    };

    index
        .tokenizers()
        .register("cang_jie", CangJieTokenizer::default());
    Ok(index)
}

impl SdsIndex {
    pub fn open_readonly(data_dir: &Path) -> anyhow::Result<Self> {
        let index = open_or_create_index(data_dir)?;
        let schema = index.schema();
        let fields = SdsFields {
            id: schema.get_field("id")?,
            text: schema.get_field("text")?,
            text_lower: schema.get_field("text_lower")?,
            source: schema.get_field("source")?,
            tags: schema.get_field("tags")?,
            created_at: schema.get_field("created_at")?,
        };
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        Ok(Self {
            index,
            fields,
            reader,
            data_dir: data_dir.to_path_buf(),
        })
    }

    pub fn search(
        &self,
        query: &str,
        top_k: usize,
        tag: Option<&str>,
        source: Option<&str>,
    ) -> anyhow::Result<Vec<Memory>> {
        use tantivy::collector::TopDocs;

        let searcher = self.reader.searcher();
        let mut sub_queries: Vec<(Box<dyn tantivy::query::Query>, Occur)> = Vec::new();

        let text_parser = QueryParser::for_index(&self.index, vec![self.fields.text_lower]);
        sub_queries.push((
            parse_query_with_literal_fallback(&text_parser, &query.to_lowercase())?,
            Occur::Must,
        ));

        if let Some(tag) = tag {
            let parser = QueryParser::for_index(&self.index, vec![self.fields.tags]);
            sub_queries.push((
                parse_query_with_literal_fallback(&parser, tag)?,
                Occur::Must,
            ));
        }
        if let Some(source) = source {
            let parser = QueryParser::for_index(&self.index, vec![self.fields.source]);
            sub_queries.push((
                parse_query_with_literal_fallback(&parser, source)?,
                Occur::Must,
            ));
        }

        let query = if sub_queries.len() == 1 {
            sub_queries.pop().expect("查询列表非空").0
        } else {
            let ordered = sub_queries
                .into_iter()
                .map(|(query, occur)| (occur, query))
                .collect();
            Box::new(BooleanQuery::new(ordered))
        };

        let top_docs =
            searcher.search(&query, &TopDocs::with_limit(top_k.max(1)).order_by_score())?;
        self.build_memory_list(&top_docs)
    }

    pub fn list(
        &self,
        limit: usize,
        offset: usize,
        source: Option<&str>,
    ) -> anyhow::Result<Vec<Memory>> {
        use tantivy::Order;
        use tantivy::collector::TopDocs;

        let searcher = self.reader.searcher();
        let query = if let Some(source) = source {
            let parser = QueryParser::for_index(&self.index, vec![self.fields.source]);
            Box::new(parser.parse_query(source)?) as Box<dyn tantivy::query::Query>
        } else {
            Box::new(AllQuery)
        };

        let top_docs = searcher.search(
            &query,
            &TopDocs::with_limit((offset + limit).max(1))
                .order_by_fast_field::<f64>("created_at", Order::Desc),
        )?;

        let mut results = Vec::new();
        for (_, address) in top_docs.iter().skip(offset).take(limit) {
            let document = searcher.doc::<TantivyDocument>(*address)?;
            results.push(self.doc_to_memory(&document));
        }
        Ok(results)
    }

    pub fn status(&self) -> SdsStatus {
        let index_dir = self.data_dir.join(INDEX_DIR);
        SdsStatus {
            memories: self.reader.searcher().num_docs(),
            index_size: dir_size(&index_dir),
            index_path: index_dir.to_string_lossy().to_string(),
        }
    }

    /// 导出所有活文档，按 id 正序。
    pub fn export_all(&self) -> anyhow::Result<Vec<Memory>> {
        let searcher = self.reader.searcher();
        let mut results = Vec::new();
        let mut seen_ids = std::collections::BTreeSet::new();

        for (segment_ord, segment_reader) in searcher.segment_readers().iter().enumerate() {
            for doc_id in 0..segment_reader.max_doc() {
                let address = tantivy::DocAddress {
                    segment_ord: segment_ord as u32,
                    doc_id,
                };
                let document = match searcher.doc::<TantivyDocument>(address) {
                    Ok(document) => document,
                    Err(_) => continue,
                };
                let id = self.get_u64(&document, self.fields.id);
                if id != 0 && seen_ids.insert(id) {
                    results.push(self.doc_to_memory(&document));
                }
            }
        }
        results.sort_by_key(|memory| memory.id);
        Ok(results)
    }

    pub fn all(&self) -> anyhow::Result<Vec<Memory>> {
        use tantivy::Order;
        use tantivy::collector::TopDocs;

        let searcher = self.reader.searcher();
        let limit = searcher.num_docs() as usize;
        if limit == 0 {
            return Ok(Vec::new());
        }
        let top_docs = searcher.search(
            &AllQuery,
            &TopDocs::with_limit(limit).order_by_fast_field::<f64>("created_at", Order::Desc),
        )?;
        top_docs
            .into_iter()
            .map(|(_, address)| {
                let document = searcher.doc::<TantivyDocument>(address)?;
                Ok(self.doc_to_memory(&document))
            })
            .collect()
    }

    pub fn max_id(&self) -> anyhow::Result<u64> {
        let path = self.data_dir.join(COUNTER_FILE);
        if !path.exists() {
            return Ok(0);
        }
        let value = std::fs::read_to_string(path)?;
        Ok(value.trim().parse::<u64>().unwrap_or(0))
    }

    pub fn segment_ids(&self) -> Vec<SegmentId> {
        self.reader
            .searcher()
            .segment_readers()
            .iter()
            .map(|segment_reader| segment_reader.segment_id())
            .collect()
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    fn get_u64(&self, document: &TantivyDocument, field: Field) -> u64 {
        document
            .get_first(field)
            .map(|value| value.as_u64().unwrap_or(0))
            .unwrap_or(0)
    }

    fn get_str(&self, document: &TantivyDocument, field: Field) -> String {
        document
            .get_first(field)
            .map(|value| value.as_str().unwrap_or("").to_string())
            .unwrap_or_default()
    }

    fn get_f64(&self, document: &TantivyDocument, field: Field) -> f64 {
        document
            .get_first(field)
            .map(|value| value.as_f64().unwrap_or(0.0))
            .unwrap_or(0.0)
    }

    fn doc_to_memory(&self, document: &TantivyDocument) -> Memory {
        Memory {
            id: self.get_u64(document, self.fields.id),
            text: self.get_str(document, self.fields.text),
            source: self.get_str(document, self.fields.source),
            tags: self.get_str(document, self.fields.tags),
            created_at: self.get_f64(document, self.fields.created_at),
            score: None,
        }
    }

    fn build_memory_list(
        &self,
        top_docs: &[(f32, tantivy::DocAddress)],
    ) -> anyhow::Result<Vec<Memory>> {
        let searcher = self.reader.searcher();
        let mut results = Vec::with_capacity(top_docs.len());
        for (score, address) in top_docs {
            let document = searcher.doc::<TantivyDocument>(*address)?;
            let mut memory = self.doc_to_memory(&document);
            memory.score = Some(f64::from(*score));
            results.push(memory);
        }
        Ok(results)
    }
}

impl SdsWriter {
    pub fn open(data_dir: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let lock_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(data_dir.join(LOCK_FILE))?;
        lock_file
            .try_lock_exclusive()
            .map_err(|_| anyhow::anyhow!("无法获取写锁，可能有其他写入进程正在使用"))?;

        let index = SdsIndex::open_readonly(data_dir)?;
        let writer = index.index.writer(WRITER_MEMORY_BUDGET)?;
        // CLI进程生命周期很短，后台合并来不及完成；统一由SDS显式管理段生命周期。
        writer.set_merge_policy(Box::new(NoMergePolicy));
        Ok(Self {
            index,
            writer,
            _lock_file: lock_file,
        })
    }

    pub fn store(&mut self, text: &str, source: &str, tags: &str) -> anyhow::Result<Memory> {
        let memory = self.write_doc(text, source, tags)?;
        self.commit()?;
        Ok(memory)
    }

    /// 批量写入，不自动提交；调用方完成批次后必须调用 commit。
    pub fn batch_store(&mut self, text: &str, source: &str, tags: &str) -> anyhow::Result<Memory> {
        self.write_doc(text, source, tags)
    }

    pub fn commit(&mut self) -> anyhow::Result<()> {
        self.flush()?;
        if self.segment_ids().len() > AUTO_COMPACT_SEGMENT_THRESHOLD {
            self.merge_all_segments()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> anyhow::Result<()> {
        self.writer.commit()?;
        self.index.reader.reload()?;
        Ok(())
    }

    pub fn delete(&mut self, id: u64) -> anyhow::Result<bool> {
        use tantivy::collector::Count;

        let term = tantivy::Term::from_field_u64(self.index.fields.id, id);
        let query = TermQuery::new(term, IndexRecordOption::Basic);
        if self.index.reader.searcher().search(&query, &Count)? == 0 {
            return Ok(false);
        }
        self.writer.delete_query(Box::new(query))?;
        self.commit()?;
        Ok(true)
    }

    pub fn delete_by_source(&mut self, source: &str) -> anyhow::Result<u64> {
        use tantivy::collector::Count;

        let parser = QueryParser::for_index(&self.index.index, vec![self.index.fields.source]);
        let query = parser.parse_query(source)?;
        let count = self.index.reader.searcher().search(&query, &Count)?;
        if count == 0 {
            return Ok(0);
        }
        self.writer.delete_query(query)?;
        self.commit()?;
        Ok(count as u64)
    }

    /// 将索引中所有可搜索段合并为一个段，并回收旧段文件。
    pub fn compact(&mut self) -> anyhow::Result<CompactStats> {
        let started_at = std::time::Instant::now();
        let index_dir = self.index.data_dir.join(INDEX_DIR);

        self.flush()?;

        let segments_before = self.segment_ids().len();
        let files_before = count_files(&index_dir);
        let size_before = dir_size_u64(&index_dir);
        let memories = self.reader.searcher().num_docs();

        let merge_operations = self.merge_all_segments()?;

        let segments_after = self.segment_ids().len();
        let files_after = count_files(&index_dir);
        let size_after = dir_size_u64(&index_dir);

        Ok(CompactStats {
            segments_before,
            segments_after,
            files_before,
            files_after,
            size_before: fmt_bytes(size_before),
            size_after: fmt_bytes(size_after),
            memories,
            merge_operations,
            elapsed_ms: started_at.elapsed().as_millis() as u64,
        })
    }

    fn merge_all_segments(&mut self) -> anyhow::Result<usize> {
        let mut merge_operations = 0;
        while self.segment_ids().len() > 1 {
            let ids = self.segment_ids();
            for batch in ids.chunks(COMPACT_MERGE_BATCH_SIZE) {
                if batch.len() > 1 {
                    self.writer.merge(batch).wait()?;
                    merge_operations += 1;
                }
            }
            // 每轮提交并刷新，下一轮基于最新段列表继续归并。
            self.flush()?;
        }
        Ok(merge_operations)
    }

    pub fn set_counter(&self, value: u64) -> anyhow::Result<()> {
        let path = self.index.data_dir.join(COUNTER_FILE);
        let temporary_path = self.index.data_dir.join(".counter.tmp");
        {
            let mut temporary = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&temporary_path)?;
            temporary.write_all(value.to_string().as_bytes())?;
            temporary.sync_all()?;
        }
        std::fs::rename(temporary_path, path)?;
        Ok(())
    }

    fn next_id(&self) -> anyhow::Result<u64> {
        let id = self.index.max_id()?.saturating_add(1);
        self.set_counter(id)?;
        Ok(id)
    }

    fn write_doc(&mut self, text: &str, source: &str, tags: &str) -> anyhow::Result<Memory> {
        let id = self.next_id()?;
        let created_at = chrono::Utc::now().timestamp() as f64;
        let mut document = TantivyDocument::default();
        document.add_u64(self.index.fields.id, id);
        document.add_text(self.index.fields.text, text);
        document.add_text(self.index.fields.text_lower, text.to_lowercase());
        document.add_text(self.index.fields.source, source);
        document.add_text(self.index.fields.tags, tags);
        document.add_f64(self.index.fields.created_at, created_at);
        self.writer.add_document(document)?;

        Ok(Memory {
            id,
            text: text.to_string(),
            source: source.to_string(),
            tags: tags.to_string(),
            created_at,
            score: None,
        })
    }
}

impl Deref for SdsWriter {
    type Target = SdsIndex;

    fn deref(&self) -> &Self::Target {
        &self.index
    }
}

#[derive(serde::Serialize, Debug)]
pub struct SdsStatus {
    pub memories: u64,
    pub index_size: String,
    pub index_path: String,
}

#[derive(serde::Serialize, Debug)]
pub struct CompactStats {
    pub segments_before: usize,
    pub segments_after: usize,
    pub files_before: usize,
    pub files_after: usize,
    pub size_before: String,
    pub size_after: String,
    pub memories: u64,
    pub merge_operations: usize,
    pub elapsed_ms: u64,
}

fn fmt_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn count_files(path: &Path) -> usize {
    let mut count = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += count_files(&path);
            } else {
                count += 1;
            }
        }
    }
    count
}

fn dir_size(path: &Path) -> String {
    fmt_bytes(dir_size_u64(path))
}

fn dir_size_u64(path: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                total += dir_size_u64(&path);
            } else {
                total += entry.metadata().map(|metadata| metadata.len()).unwrap_or(0);
            }
        }
    }
    total
}
