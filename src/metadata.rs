use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const METADATA_FILE: &str = "metadata.json";
const METADATA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryMeta {
    pub memory_type: String,
    pub importance: f64,
    pub text_hash: String,
    pub updated_at: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct MetadataFile {
    version: u32,
    entries: BTreeMap<u64, MemoryMeta>,
}

pub struct MetadataStore {
    path: PathBuf,
    entries: BTreeMap<u64, MemoryMeta>,
}

impl MetadataStore {
    pub fn open(data_dir: &Path) -> anyhow::Result<Self> {
        let path = data_dir.join(METADATA_FILE);
        if !path.exists() {
            return Ok(Self {
                path,
                entries: BTreeMap::new(),
            });
        }
        let file: MetadataFile = serde_json::from_slice(&std::fs::read(&path)?)?;
        if file.version > METADATA_VERSION {
            anyhow::bail!(
                "元数据版本 {} 高于当前支持版本 {}",
                file.version,
                METADATA_VERSION
            );
        }
        Ok(Self {
            path,
            entries: file.entries,
        })
    }

    pub fn get_or_default(&self, id: u64) -> MemoryMeta {
        self.entries
            .get(&id)
            .cloned()
            .unwrap_or_else(|| MemoryMeta {
                memory_type: "legacy".to_string(),
                importance: 50.0,
                text_hash: String::new(),
                updated_at: 0.0,
            })
    }

    pub fn insert(&mut self, id: u64, meta: MemoryMeta) {
        self.entries.insert(id, meta);
    }

    pub fn remove(&mut self, id: u64) {
        self.entries.remove(&id);
    }

    pub fn find_hash(&self, hash: &str) -> Option<u64> {
        self.entries
            .iter()
            .find_map(|(id, meta)| (meta.text_hash == hash).then_some(*id))
    }

    pub fn persist(&self) -> anyhow::Result<()> {
        let temp_path = self.path.with_extension("tmp");
        let payload = MetadataFile {
            version: METADATA_VERSION,
            entries: self.entries.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&payload)?;
        {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&temp_path)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        std::fs::rename(temp_path, &self.path)?;
        Ok(())
    }
}

pub fn text_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.trim().as_bytes());
    format!("{:x}", hasher.finalize())
}
