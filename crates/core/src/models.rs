use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NoteType {
    Semantique,
    Episodique,
    Procedural,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NoteStatus {
    Actif,
    Obsolete,
    Archive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteMetadata {
    pub id: String,
    pub title: String,
    pub date_creation: String,
    pub date_modification: String,
    pub note_type: NoteType,
    pub statut: NoteStatus,
    pub tags: Vec<String>,
    pub source_media: Option<String>,
}
