use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Types de mémoire CoALA (Cognitive Architectures for Language Agents).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CoalaType {
    #[serde(alias = "procedural", alias = "procedure", alias = "procedurale")]
    Procedural,
    #[serde(alias = "episodic", alias = "episodique")]
    Episodic,
    #[default]
    #[serde(alias = "semantic", alias = "semantique")]
    Semantic,
}

impl CoalaType {
    /// Nom canonique en anglais pour la persistance SQL (`chunks.coala_type`).
    #[inline]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Procedural => "procedural",
            Self::Episodic => "episodic",
            Self::Semantic => "semantic",
        }
    }

    /// Nom en français pour la rétrocompatibilité d'affichage.
    #[inline]
    pub fn as_french_str(&self) -> &'static str {
        match self {
            Self::Procedural => "procedural",
            Self::Episodic => "episodique",
            Self::Semantic => "semantique",
        }
    }

    /// Coefficient d'atténuation temporelle lambda (SPEC §2.3) :
    /// - lambda = 0.0 pour procedural (immunité totale au vieillissement)
    /// - lambda = 0.005 pour semantic et episodic (atténuation temporelle progressive)
    #[inline]
    pub fn decay_lambda(&self) -> f64 {
        match self {
            Self::Procedural => 0.0,
            Self::Episodic | Self::Semantic => 0.005,
        }
    }

    /// Parse tolérant acceptant les variantes anglaises et françaises.
    pub fn from_str_lenient(s: &str) -> Self {
        let normalized = s.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "procedural" | "procedure" | "procedurale" => Self::Procedural,
            "episodic" | "episodique" => Self::Episodic,
            _ => Self::Semantic,
        }
    }
}

impl fmt::Display for CoalaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for CoalaType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "procedural" | "procedure" | "procedurale" => Ok(Self::Procedural),
            "episodic" | "episodique" => Ok(Self::Episodic),
            "semantic" | "semantique" => Ok(Self::Semantic),
            _ => Err(format!("Type CoALA inconnu: '{s}'")),
        }
    }
}

impl rusqlite::types::ToSql for CoalaType {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Borrowed(
            rusqlite::types::ValueRef::Text(self.as_str().as_bytes()),
        ))
    }
}

impl rusqlite::types::FromSql for CoalaType {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = value.as_str()?;
        s.parse::<Self>()
            .map_err(|e| rusqlite::types::FromSqlError::Other(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e))))
    }
}

/// Statut d'obsolescence et de cycle de vie d'une note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NoteStatus {
    #[default]
    #[serde(alias = "active", alias = "actif")]
    Active,
    #[serde(alias = "deprecated", alias = "obsolete", alias = "archive", alias = "archived")]
    Deprecated,
}

impl NoteStatus {
    /// Nom canonique en anglais pour la persistance SQL (`chunks.status`).
    #[inline]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Deprecated => "deprecated",
        }
    }

    /// Nom en français pour la rétrocompatibilité d'affichage.
    #[inline]
    pub fn as_french_str(&self) -> &'static str {
        match self {
            Self::Active => "actif",
            Self::Deprecated => "obsolete",
        }
    }

    /// Vérifie si la note est active (doit être incluse dans la recherche RAG standard).
    #[inline]
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }

    /// Vérifie si la note est dépréciée (exclue de la recherche standard).
    #[inline]
    pub fn is_deprecated(&self) -> bool {
        matches!(self, Self::Deprecated)
    }

    /// Parse tolérant acceptant les variantes anglaises et françaises.
    pub fn from_str_lenient(s: &str) -> Self {
        let normalized = s.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "deprecated" | "obsolete" | "archive" | "archived" => Self::Deprecated,
            _ => Self::Active,
        }
    }
}

impl fmt::Display for NoteStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for NoteStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "active" | "actif" => Ok(Self::Active),
            "deprecated" | "obsolete" | "archive" | "archived" => Ok(Self::Deprecated),
            _ => Err(format!("Statut de note inconnu: '{s}'")),
        }
    }
}

impl rusqlite::types::ToSql for NoteStatus {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Borrowed(
            rusqlite::types::ValueRef::Text(self.as_str().as_bytes()),
        ))
    }
}

impl rusqlite::types::FromSql for NoteStatus {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = value.as_str()?;
        s.parse::<Self>()
            .map_err(|e| rusqlite::types::FromSqlError::Other(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e))))
    }
}

/// Relation de graphe 1-hop entre fichiers du coffre (wikilinks, relations d'obsolescence).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileLink {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    pub source_path: String,
    pub target_path: String,
    pub link_type: String,
    pub created_at: i64,
}

impl FileLink {
    pub const TYPE_WIKILINK: &'static str = "wikilink";
    pub const TYPE_SUPERSEDES: &'static str = "supersedes";
    pub const TYPE_RELATES: &'static str = "relates";

    pub fn new(
        source_path: impl Into<String>,
        target_path: impl Into<String>,
        link_type: impl Into<String>,
        created_at: i64,
    ) -> Self {
        Self {
            id: None,
            source_path: source_path.into(),
            target_path: target_path.into(),
            link_type: link_type.into(),
            created_at,
        }
    }

    pub fn with_id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }
}

/// Résultat enrichi d'une recherche RAG hybride combinant similarité vectorielle et FTS5.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HybridSearchResult {
    pub chunk_id: String,
    pub file_path: String,
    pub content: String,
    pub vector_score: f64,
    pub bm25_score: f64,
    pub combined_score: f64,
    pub coala_type: CoalaType,
    pub status: NoteStatus,
    pub superseded_by: Option<String>,
    pub deprecated_at: Option<i64>,
    pub age_days: f64,
}

fn default_note_type() -> String {
    "semantique".to_string()
}

fn default_statut() -> String {
    "actif".to_string()
}

/// Métadonnées d'en-tête (frontmatter YAML) extraites d'une note Markdown.
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
    #[serde(alias = "type", alias = "coala_type", default = "default_note_type")]
    pub note_type: String,
    #[serde(alias = "status", default = "default_statut")]
    pub statut: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub source_media: Option<String>,
    #[serde(alias = "supersedes", default)]
    pub superseded_by: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_timestamp")]
    pub deprecated_at: Option<i64>,
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
            superseded_by: None,
            deprecated_at: None,
        }
    }
}

impl NoteFrontmatter {
    /// Résout le type CoALA typé à partir de la chaîne `note_type`.
    #[inline]
    pub fn coala_type(&self) -> CoalaType {
        CoalaType::from_str_lenient(&self.note_type)
    }

    /// Résout le statut typé à partir de la chaîne `statut`.
    #[inline]
    pub fn status(&self) -> NoteStatus {
        NoteStatus::from_str_lenient(&self.statut)
    }

    /// Parse la date de création en timestamp Unix (secondes),
    /// supportant les formats RFC 3339, YYYY-MM-DD ou entier brut.
    pub fn parse_date_creation(&self) -> Option<i64> {
        let s = self.date_creation.trim();
        if s.is_empty() {
            return None;
        }
        if let Ok(ts) = s.parse::<i64>() {
            return Some(ts);
        }
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
            return Some(dt.timestamp());
        }
        if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
            return d.and_hms_opt(0, 0, 0).map(|dt| dt.and_utc().timestamp());
        }
        None
    }

    /// Nettoie la cible `superseded_by` en retirant les délimiteurs wikilink `[[...]]` si présents.
    pub fn clean_superseded_by(&self) -> Option<String> {
        self.superseded_by.as_deref().map(|s| {
            let trimmed = s.trim();
            if trimmed.starts_with("[[") && trimmed.ends_with("]]") && trimmed.len() >= 4 {
                trimmed[2..trimmed.len() - 2].trim().to_string()
            } else {
                trimmed.to_string()
            }
        })
    }
}

struct OptionalTimestampVisitor;

impl<'de> serde::de::Visitor<'de> for OptionalTimestampVisitor {
    type Value = Option<i64>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a timestamp integer, date/time string, or null")
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(None)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(None)
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Some(v))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Some(v as i64))
    }

    fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        if let Ok(ts) = trimmed.parse::<i64>() {
            return Ok(Some(ts));
        }
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(trimmed) {
            return Ok(Some(dt.timestamp()));
        }
        if let Ok(d) = chrono::NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
            return Ok(d.and_hms_opt(0, 0, 0).map(|dt| dt.and_utc().timestamp()));
        }
        Ok(None)
    }
}

fn deserialize_optional_timestamp<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserializer.deserialize_option(OptionalTimestampVisitor)
}

/// Fragment de note indexé dans SQLite (`chunks` et `vec_chunks`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexedChunk {
    /// Identifiant entier unique (rowid SQLite 64-bit mappé 1:1 avec `vec_chunks.rowid`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    pub chunk_id: String,
    pub file_path: String,
    pub chunk_index: usize,
    pub content: String,
    pub token_count: usize,
    #[serde(alias = "note_type", default)]
    pub coala_type: CoalaType,
    #[serde(alias = "statut", default)]
    pub status: NoteStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated_at: Option<i64>,
    pub date_creation: i64,
}

impl IndexedChunk {
    /// Constructeur principal
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        chunk_id: impl Into<String>,
        file_path: impl Into<String>,
        chunk_index: usize,
        content: impl Into<String>,
        token_count: usize,
        coala_type: CoalaType,
        status: NoteStatus,
        date_creation: i64,
    ) -> Self {
        Self {
            id: None,
            chunk_id: chunk_id.into(),
            file_path: file_path.into(),
            chunk_index,
            content: content.into(),
            token_count,
            coala_type,
            status,
            superseded_by: None,
            deprecated_at: None,
            date_creation,
        }
    }

    /// Accesseur de rétrocompatibilité pour `note_type` (retourne &str)
    #[inline]
    pub fn note_type(&self) -> &str {
        self.coala_type.as_str()
    }

    /// Accesseur de rétrocompatibilité pour `statut` (retourne &str)
    #[inline]
    pub fn statut(&self) -> &str {
        self.status.as_str()
    }

    /// Nom canonique pour CoALA
    #[inline]
    pub fn coala_type_str(&self) -> &str {
        self.coala_type.as_str()
    }

    /// Nom canonique pour statut
    #[inline]
    pub fn status_str(&self) -> &str {
        self.status.as_str()
    }

    /// Builder d'obsolescence
    pub fn with_obsolescence(
        mut self,
        superseded_by: Option<String>,
        deprecated_at: Option<i64>,
    ) -> Self {
        self.superseded_by = superseded_by;
        self.deprecated_at = deprecated_at;
        self
    }

    /// Builder d'identifiant rowid
    pub fn with_id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }
}

impl Default for IndexedChunk {
    fn default() -> Self {
        Self {
            id: None,
            chunk_id: String::new(),
            file_path: String::new(),
            chunk_index: 0,
            content: String::new(),
            token_count: 0,
            coala_type: CoalaType::Semantic,
            status: NoteStatus::Active,
            superseded_by: None,
            deprecated_at: None,
            date_creation: 0,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coala_type_serde_and_aliases() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(serde_json::to_string(&CoalaType::Procedural)?, "\"procedural\"");
        assert_eq!(serde_json::to_string(&CoalaType::Episodic)?, "\"episodic\"");
        assert_eq!(serde_json::to_string(&CoalaType::Semantic)?, "\"semantic\"");

        let p: CoalaType = serde_json::from_str("\"procedurale\"")?;
        assert_eq!(p, CoalaType::Procedural);
        assert_eq!(p.decay_lambda(), 0.0);

        let e: CoalaType = serde_json::from_str("\"episodique\"")?;
        assert_eq!(e, CoalaType::Episodic);
        assert_eq!(e.decay_lambda(), 0.005);

        let s: CoalaType = serde_json::from_str("\"semantique\"")?;
        assert_eq!(s, CoalaType::Semantic);
        assert_eq!(s.decay_lambda(), 0.005);
        Ok(())
    }

    #[test]
    fn test_note_status_serde_and_aliases() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(serde_json::to_string(&NoteStatus::Active)?, "\"active\"");
        assert_eq!(serde_json::to_string(&NoteStatus::Deprecated)?, "\"deprecated\"");

        let a: NoteStatus = serde_json::from_str("\"actif\"")?;
        assert!(a.is_active());

        let d1: NoteStatus = serde_json::from_str("\"obsolete\"")?;
        assert!(d1.is_deprecated());

        let d2: NoteStatus = serde_json::from_str("\"archive\"")?;
        assert!(d2.is_deprecated());
        Ok(())
    }

    #[test]
    fn test_file_link_creation() {
        let link = FileLink::new("a.md", "b.md", FileLink::TYPE_SUPERSEDES, 1710000000);
        assert_eq!(link.link_type, "supersedes");
        assert_eq!(link.source_path, "a.md");
        assert_eq!(link.target_path, "b.md");
        assert!(link.id.is_none());

        let link_with_id = link.with_id(42);
        assert_eq!(link_with_id.id, Some(42));
    }

    #[test]
    fn test_indexed_chunk_compatibility() -> Result<(), Box<dyn std::error::Error>> {
        let json_legacy = r#"{
            "chunk_id": "c1",
            "file_path": "n.md",
            "chunk_index": 0,
            "content": "abc",
            "token_count": 3,
            "note_type": "semantique",
            "statut": "actif",
            "date_creation": 1710000000
        }"#;

        let chunk: IndexedChunk = serde_json::from_str(json_legacy)?;
        assert_eq!(chunk.coala_type, CoalaType::Semantic);
        assert_eq!(chunk.status, NoteStatus::Active);
        assert_eq!(chunk.note_type(), "semantic");
        assert_eq!(chunk.statut(), "active");
        assert!(chunk.id.is_none());
        assert!(chunk.superseded_by.is_none());
        Ok(())
    }

    #[test]
    fn test_frontmatter_enhanced_parsing() -> Result<(), Box<dyn std::error::Error>> {
        let yaml = r#"---
id: test-1
title: Test Title
type: procedural
status: deprecated
supersedes: "[[Notes/OldNote.md]]"
deprecated_at: "2026-09-12T10:00:00Z"
date_creation: "2026-01-15T08:00:00Z"
---
Body content"#;
        let matter = gray_matter::Matter::<gray_matter::engine::YAML>::new();
        let parsed = matter
            .parse_with_struct::<NoteFrontmatter>(yaml)
            .ok_or("parse error")?;
        let fm = parsed.data;

        assert_eq!(fm.coala_type(), CoalaType::Procedural);
        assert_eq!(fm.status(), NoteStatus::Deprecated);
        assert_eq!(fm.clean_superseded_by(), Some("Notes/OldNote.md".to_string()));
        assert_eq!(fm.deprecated_at, Some(1789207200));
        assert_eq!(fm.parse_date_creation(), Some(1768464000));
        Ok(())
    }
}
