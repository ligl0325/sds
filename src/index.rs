use std::path::{Path, PathBuf};

use tantivy::query::{AllQuery, BooleanQuery, Occur, QueryParser, TermQuery};
use tantivy::schema::*;
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument};

use cang_jie::CangJieTokenizer;
const COUNTER_FILE: &str = "counter";
const INDEX_DIR: &str = "tantivy_index";

#[derive(serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct Memory {
    pub id: u64,
    pub text: String,
    pub source: String,
    pub tags: String,
    pub created_at: f64,
    pub score: Option<f64>,
}

pub struct SdsIndex {
    index: Index,
    fields: SdsFields,
    reader: IndexReader,
    writer: IndexWriter,
    data_dir: PathBuf,
}

struct SdsFields {
    id: Field,
    text: Field,
    text_lower: Field,
    source: Field,
    tags: Field,
    created_at: Field,
}

// Tantivy tokenizer option shared by source/tags
fn jieba_text_option() -> TextOptions {
    TextOptions::default().set_stored().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer("cang_jie")
            .set_index_option(IndexRecordOption::WithFreqsAndPositions)
            .set_fieldnorms(true),
    )
}

impl SdsIndex {
    pub fn open(data_dir: &Path) -> anyhow::Result<Self> {
        let index_dir = data_dir.join(INDEX_DIR);
        std::fs::create_dir_all(&index_dir)?;

        let mut schema_builder = Schema::builder();
        schema_builder.add_u64_field("id", FAST | STORED);
        schema_builder.add_text_field("text", STRING | STORED);
        schema_builder.add_text_field("text_lower", jieba_text_option());
        schema_builder.add_text_field("source", jieba_text_option());
        schema_builder.add_text_field("tags", jieba_text_option());
        schema_builder.add_f64_field("created_at", FAST | STORED);
        let schema = schema_builder.build();

        let index = if index_dir.join("meta.json").exists() {
            Index::open_in_dir(&index_dir)?
        } else {
            Index::create_in_dir(&index_dir, schema.clone())?
        };

        index
            .tokenizers()
            .register("cang_jie", CangJieTokenizer::default());

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        let writer = index.writer(50_000_000)?;

        // Verify schema fields exist
        let fields = SdsFields {
            id: schema.get_field("id")?,
            text: schema.get_field("text")?,
            text_lower: schema.get_field("text_lower")?,
            source: schema.get_field("source")?,
            tags: schema.get_field("tags")?,
            created_at: schema.get_field("created_at")?,
        };

        Ok(Self {
            index,
            fields,
            reader,
            writer,
            data_dir: data_dir.to_path_buf(),
        })
    }

    fn next_id(&self) -> anyhow::Result<u64> {
        let counter_path = self.data_dir.join(COUNTER_FILE);
        let id = if counter_path.exists() {
            let s = std::fs::read_to_string(&counter_path)?.trim().to_string();
            s.parse::<u64>().unwrap_or(0) + 1
        } else {
            1
        };
        std::fs::write(&counter_path, id.to_string())?;
        Ok(id)
    }

    pub fn store(&mut self, text: &str, source: &str, tags: &str) -> anyhow::Result<Memory> {
        let mem = self.write_doc(text, source, tags)?;
        self.writer.commit()?;
        self.reader.reload()?;
        Ok(mem)
    }

    /// 批量写入（不加 commit，调用方负责 commit）
    pub fn batch_store(&mut self, text: &str, source: &str, tags: &str) -> anyhow::Result<Memory> {
        self.write_doc(text, source, tags)
    }

    fn write_doc(&mut self, text: &str, source: &str, tags: &str) -> anyhow::Result<Memory> {
        let id = self.next_id()?;
        let now = chrono::Utc::now().timestamp() as f64;

        let mut doc = TantivyDocument::default();
        doc.add_u64(self.fields.id, id);
        doc.add_text(self.fields.text, text);
        doc.add_text(self.fields.text_lower, text.to_lowercase());
        doc.add_text(self.fields.source, source);
        doc.add_text(self.fields.tags, tags);
        doc.add_f64(self.fields.created_at, now);

        self.writer.add_document(doc)?;

        Ok(Memory {
            id,
            text: text.to_string(),
            source: source.to_string(),
            tags: tags.to_string(),
            created_at: now,
            score: None,
        })
    }

    /// 搜索记忆（BM25），支持 tag / source 过滤（AND 组合）
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

        // text_lower 搜索
        let text_parser = QueryParser::for_index(&self.index, vec![self.fields.text_lower]);
        sub_queries.push((text_parser.parse_query(&query.to_lowercase())?, Occur::Must));

        // tag 过滤
        if let Some(t) = tag {
            let tag_parser = QueryParser::for_index(&self.index, vec![self.fields.tags]);
            sub_queries.push((tag_parser.parse_query(t)?, Occur::Must));
        }

        // source 过滤
        if let Some(s) = source {
            let source_parser = QueryParser::for_index(&self.index, vec![self.fields.source]);
            sub_queries.push((source_parser.parse_query(s)?, Occur::Must));
        }

        let query_obj = if sub_queries.len() == 1 {
            sub_queries.pop().unwrap().0
        } else {
            // BooleanQuery 需要 (Occur, Box<dyn Query>) 顺序
            let ordered: Vec<(Occur, Box<dyn tantivy::query::Query>)> =
                sub_queries.into_iter().map(|(q, o)| (o, q)).collect();
            Box::new(BooleanQuery::new(ordered))
        };

        let top_docs = searcher.search(
            &query_obj,
            &TopDocs::with_limit(top_k.max(1)).order_by_score(),
        )?;

        self.build_memory_list(&top_docs)
    }

    /// 列出记忆，支持 source 过滤
    pub fn list(
        &self,
        limit: usize,
        offset: usize,
        source: Option<&str>,
    ) -> anyhow::Result<Vec<Memory>> {
        use tantivy::Order;
        use tantivy::collector::TopDocs;

        let searcher = self.reader.searcher();

        let query_obj = if let Some(s) = source {
            let source_parser = QueryParser::for_index(&self.index, vec![self.fields.source]);
            Box::new(source_parser.parse_query(s)?) as Box<dyn tantivy::query::Query>
        } else {
            Box::new(AllQuery)
        };

        let fetch = (offset + limit).max(1);
        let top_docs = searcher.search(
            &query_obj,
            &TopDocs::with_limit(fetch).order_by_fast_field::<f64>("created_at", Order::Desc),
        )?;

        let mut results = Vec::new();
        for (_, doc_addr) in top_docs.iter().skip(offset).take(limit) {
            let doc = searcher.doc::<TantivyDocument>(*doc_addr)?;
            results.push(self.doc_to_memory(&doc));
        }

        Ok(results)
    }

    pub fn delete(&mut self, id: u64) -> anyhow::Result<bool> {
        use tantivy::collector::Count;
        let term = tantivy::Term::from_field_u64(self.fields.id, id);
        let query = TermQuery::new(term, tantivy::schema::IndexRecordOption::Basic);

        let searcher = self.reader.searcher();
        let exists = searcher.search(&query, &Count)? > 0;
        if !exists {
            return Ok(false);
        }

        let _ = self.writer.delete_query(Box::new(query));
        self.writer.commit()?;
        self.reader.reload()?;
        Ok(true)
    }

    /// 按 source 批量删除所有匹配记录
    pub fn delete_by_source(&mut self, source: &str) -> anyhow::Result<u64> {
        use tantivy::collector::Count;
        let source_parser = QueryParser::for_index(&self.index, vec![self.fields.source]);
        let query = source_parser.parse_query(source)?;
        let searcher = self.reader.searcher();
        let count = searcher.search(&query, &Count)?;
        if count == 0 {
            return Ok(0);
        }
        let _ = self.writer.delete_query(Box::new(query));
        self.writer.commit()?;
        self.reader.reload()?;
        Ok(count as u64)
    }

    pub fn status(&self) -> anyhow::Result<SdsStatus> {
        let searcher = self.reader.searcher();
        let num_docs = searcher.num_docs();
        let index_dir = self.data_dir.join(INDEX_DIR);
        let index_size = dir_size(&index_dir);

        Ok(SdsStatus {
            memories: num_docs,
            index_size,
            index_path: index_dir.to_string_lossy().to_string(),
        })
    }

    /// 导出所有存量数据（用于迁移），按 id 正序
    pub fn export_all(&self) -> anyhow::Result<Vec<Memory>> {
        let searcher = self.reader.searcher();
        let counter = self.max_id()?;
        if counter == 0 {
            return Ok(Vec::new());
        }

        // 遍历所有活文档，逐个读取存储字段
        let mut results: Vec<Memory> = Vec::new();
        let mut seen_ids: std::collections::BTreeSet<u64> = std::collections::BTreeSet::new();

        for (segment_ord, seg_reader) in searcher.segment_readers().iter().enumerate() {
            let max_docs = seg_reader.max_doc();
            for doc_id in 0..max_docs {
                let doc_addr = tantivy::DocAddress {
                    segment_ord: segment_ord as u32,
                    doc_id,
                };
                let doc = match searcher.doc::<TantivyDocument>(doc_addr) {
                    Ok(d) => d,
                    Err(_) => continue, // 已删除或无效文档
                };
                let id = self.get_u64(&doc, self.fields.id);
                if id == 0 {
                    continue;
                }
                if seen_ids.contains(&id) {
                    continue;
                }
                seen_ids.insert(id);
                results.push(self.doc_to_memory(&doc));
            }
        }

        results.sort_by_key(|m| m.id);
        Ok(results)
    }
    pub fn all(&self) -> anyhow::Result<Vec<Memory>> {
        use tantivy::Order;
        use tantivy::collector::TopDocs;

        let searcher = self.reader.searcher();
        let counter = self.max_id()?;
        let limit = counter as usize;
        if limit == 0 {
            return Ok(Vec::new());
        }

        let top_docs = searcher.search(
            &AllQuery,
            &TopDocs::with_limit(limit).order_by_fast_field::<f64>("created_at", Order::Desc),
        )?;

        let mut results = Vec::with_capacity(limit);
        for (_, doc_addr) in &top_docs {
            let doc = searcher.doc::<TantivyDocument>(*doc_addr)?;
            results.push(self.doc_to_memory(&doc));
        }
        Ok(results)
    }

    pub fn compact(&mut self) -> anyhow::Result<()> {
        self.writer.commit()?;
        self.reader.reload()?;
        Ok(())
    }

    pub fn max_id(&self) -> anyhow::Result<u64> {
        let counter_path = self.data_dir.join(COUNTER_FILE);
        if counter_path.exists() {
            let s = std::fs::read_to_string(&counter_path)?.trim().to_string();
            Ok(s.parse::<u64>().unwrap_or(0))
        } else {
            Ok(0)
        }
    }

    pub fn writer(&mut self) -> &mut IndexWriter {
        &mut self.writer
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    // ── helpers ──

    fn get_u64(&self, doc: &TantivyDocument, field: Field) -> u64 {
        doc.get_first(field)
            .map(|v| v.as_u64().unwrap_or(0))
            .unwrap_or(0)
    }

    fn get_str(&self, doc: &TantivyDocument, field: Field) -> String {
        doc.get_first(field)
            .map(|v| v.as_str().unwrap_or("").to_string())
            .unwrap_or_default()
    }

    fn get_f64(&self, doc: &TantivyDocument, field: Field) -> f64 {
        doc.get_first(field)
            .map(|v| v.as_f64().unwrap_or(0.0))
            .unwrap_or(0.0)
    }

    fn doc_to_memory(&self, doc: &TantivyDocument) -> Memory {
        Memory {
            id: self.get_u64(doc, self.fields.id),
            text: self.get_str(doc, self.fields.text),
            source: self.get_str(doc, self.fields.source),
            tags: self.get_str(doc, self.fields.tags),
            created_at: self.get_f64(doc, self.fields.created_at),
            score: None,
        }
    }

    fn build_memory_list(
        &self,
        top_docs: &[(f32, tantivy::DocAddress)],
    ) -> anyhow::Result<Vec<Memory>> {
        let searcher = self.reader.searcher();
        let mut results = Vec::new();
        for (score, doc_addr) in top_docs {
            let doc = searcher.doc::<TantivyDocument>(*doc_addr)?;
            let mut mem = self.doc_to_memory(&doc);
            mem.score = Some(*score as f64);
            results.push(mem);
        }
        Ok(results)
    }
}

impl Drop for SdsIndex {
    fn drop(&mut self) {
        let _ = self.writer.commit();
    }
}

#[derive(serde::Serialize, Debug)]
pub struct SdsStatus {
    pub memories: u64,
    pub index_size: String,
    pub index_path: String,
}

fn dir_size(path: &PathBuf) -> String {
    let bytes = dir_size_u64(path);
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn dir_size_u64(path: &PathBuf) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                total += dir_size_u64(&path);
            } else {
                total += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    total
}
