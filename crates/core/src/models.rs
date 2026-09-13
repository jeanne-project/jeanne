use serde::{Deserialize, Serialize};

fn default_note_type() -> String {
    "semantique".to_string()
}

fn default_statut() -> String {
    "actif".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NoteFrontmatter {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub date_creation: String,
    #[serde(default)]
    pub date_modification: String,
    #[serde(alias = "type", default = "default_note_type")]
    pub note_type: String, // "semantique" | "episodique" | "procedural"
    #[serde(default = "default_statut")]
    pub statut: String, // "actif" | "obsolete" | "archive"
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub source_media: Option<String>,
}

impl Default for NoteFrontmatter {
    fn default() -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            date_creation: String::new(),
            date_modification: String::new(),
            note_type: default_note_type(),
            statut: default_statut(),
            tags: Vec::new(),
            source_media: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexedChunk {
    pub chunk_id: String,
    pub file_path: String,
    pub chunk_index: usize,
    pub content: String,
    pub token_count: usize,
    pub note_type: String,
    pub statut: String,
    pub date_creation: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchResult {
    pub chunk_id: String,
    pub file_path: String,
    pub title: String,
    pub snippet: String,
    pub score: f64,
    pub statut: String,
    pub date_creation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct VaultStats {
    pub total_files: usize,
    pub total_chunks: usize,
    pub last_scan_timestamp: i64,
}
