use crate::error::{JeanneError, Result};
use crate::models::{IndexedChunk, SearchResult, VaultStats};
use rusqlite::Connection;
use std::path::Path;
use std::sync::OnceLock;

static SQLITE_VEC_INIT: OnceLock<std::result::Result<(), i32>> = OnceLock::new();

/// Registers the `sqlite-vec` extension globally for all SQLite connections in this process.
/// Must be invoked before opening any `rusqlite::Connection`.
pub fn register_sqlite_vec() -> Result<()> {
    let res = SQLITE_VEC_INIT.get_or_init(|| {
        // SAFETY: `sqlite3_vec_init` is the official C entry point exported by `sqlite-vec`.
        // We transmute the function pointer to SQLite's `sqlite3_auto_extension` callback signature.
        // `sqlite3_auto_extension` is thread-safe and idempotent within SQLite's initialization routines.
        let rc = unsafe {
            let entry_point = std::mem::transmute::<
                *const (),
                unsafe extern "C" fn(
                    *mut rusqlite::ffi::sqlite3,
                    *mut *mut std::os::raw::c_char,
                    *const rusqlite::ffi::sqlite3_api_routines,
                ) -> std::os::raw::c_int,
            >(sqlite_vec::sqlite3_vec_init as *const ());
            rusqlite::ffi::sqlite3_auto_extension(Some(entry_point))
        };
        if rc == rusqlite::ffi::SQLITE_OK {
            Ok(())
        } else {
            Err(rc)
        }
    });

    match res {
        Ok(()) => Ok(()),
        Err(code) => Err(JeanneError::Database(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(*code),
            Some("Failed to register sqlite-vec auto extension".to_string()),
        ))),
    }
}

/// Gestionnaire de persistance locale SQLite et index FTS5.
pub struct StorageManager {
    conn: Connection,
}

impl StorageManager {
    /// Ouvre ou crée une base de données SQLite au chemin spécifié.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        register_sqlite_vec()?;
        let path_ref = path.as_ref();
        if let Some(parent) = path_ref.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(path_ref)?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            PRAGMA busy_timeout = 5000;
            "#,
        )?;
        Ok(Self { conn })
    }

    /// Ouvre une base de données en mémoire vive (pour les tests rapides).
    pub fn open_in_memory() -> Result<Self> {
        register_sqlite_vec()?;
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            PRAGMA busy_timeout = 5000;
            "#,
        )?;
        Ok(Self { conn })
    }

    /// Initialise la configuration PRAGMA et le schéma (tables, FTS5, vec_chunks, file_links, triggers).
    pub fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            PRAGMA busy_timeout = 5000;
            "#,
        )?;

        // Migration in-place si la table chunks existe sous l'ancien format sans 'id'
        self.migrate_schema_if_needed()?;

        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS files (
                file_path TEXT PRIMARY KEY,
                file_hash TEXT NOT NULL,
                last_modified INTEGER NOT NULL,
                frontmatter_json TEXT
            );

            CREATE TABLE IF NOT EXISTS chunks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chunk_id TEXT UNIQUE NOT NULL,
                file_path TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                content TEXT NOT NULL,
                token_count INTEGER NOT NULL,
                coala_type TEXT NOT NULL,
                status TEXT NOT NULL,
                superseded_by TEXT,
                deprecated_at INTEGER,
                date_creation INTEGER NOT NULL,
                FOREIGN KEY(file_path) REFERENCES files(file_path) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS file_links (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_path TEXT NOT NULL,
                target_path TEXT NOT NULL,
                link_type TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(source_path) REFERENCES files(file_path) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_chunks_file_path ON chunks(file_path);
            CREATE INDEX IF NOT EXISTS idx_chunks_coala_type ON chunks(coala_type);
            CREATE INDEX IF NOT EXISTS idx_chunks_status ON chunks(status);
            CREATE INDEX IF NOT EXISTS idx_chunks_superseded ON chunks(superseded_by);
            CREATE INDEX IF NOT EXISTS idx_file_links_source ON file_links(source_path);
            CREATE INDEX IF NOT EXISTS idx_file_links_target ON file_links(target_path);
            CREATE INDEX IF NOT EXISTS idx_file_links_type ON file_links(link_type);

            CREATE VIRTUAL TABLE IF NOT EXISTS fts_notes USING fts5(
                chunk_id UNINDEXED,
                content,
                file_path UNINDEXED,
                tokenize = 'porter unicode61'
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS vec_chunks USING vec0(
                rowid INTEGER PRIMARY KEY,
                embedding FLOAT[384] distance_metric=cosine
            );

            CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
                INSERT INTO fts_notes(chunk_id, content, file_path)
                VALUES (new.chunk_id, new.content, new.file_path);
            END;

            CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
                DELETE FROM fts_notes WHERE chunk_id = old.chunk_id;
            END;

            CREATE TRIGGER IF NOT EXISTS chunks_au AFTER UPDATE ON chunks BEGIN
                DELETE FROM fts_notes WHERE chunk_id = old.chunk_id;
                INSERT INTO fts_notes(chunk_id, content, file_path)
                VALUES (new.chunk_id, new.content, new.file_path);
            END;

            CREATE TRIGGER IF NOT EXISTS chunks_vd AFTER DELETE ON chunks BEGIN
                DELETE FROM vec_chunks WHERE rowid = old.id;
            END;
            "#,
        )?;
        Ok(())
    }

    fn migrate_schema_if_needed(&self) -> Result<()> {
        let table_exists: bool = match self.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='chunks';",
            [],
            |row| row.get::<_, i64>(0),
        ) {
            Ok(count) => count > 0,
            Err(_) => false,
        };

        if !table_exists {
            return Ok(());
        }

        let mut stmt = self.conn.prepare("PRAGMA table_info(chunks);")?;
        let mut rows = stmt.query([])?;
        let mut has_id = false;
        let mut has_coala_type = false;
        while let Some(row) = rows.next()? {
            let col_name: String = row.get(1)?;
            if col_name == "id" {
                has_id = true;
            }
            if col_name == "coala_type" {
                has_coala_type = true;
            }
        }

        if !has_id || !has_coala_type {
            self.conn.execute_batch(
                r#"
                PRAGMA foreign_keys = OFF;

                CREATE TABLE chunks_new (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    chunk_id TEXT UNIQUE NOT NULL,
                    file_path TEXT NOT NULL,
                    chunk_index INTEGER NOT NULL,
                    content TEXT NOT NULL,
                    token_count INTEGER NOT NULL,
                    coala_type TEXT NOT NULL,
                    status TEXT NOT NULL,
                    superseded_by TEXT,
                    deprecated_at INTEGER,
                    date_creation INTEGER NOT NULL,
                    FOREIGN KEY(file_path) REFERENCES files(file_path) ON DELETE CASCADE
                );

                INSERT INTO chunks_new (
                    chunk_id, file_path, chunk_index, content,
                    token_count, coala_type, status, superseded_by, deprecated_at, date_creation
                )
                SELECT
                    chunk_id, file_path, chunk_index, content,
                    token_count, note_type, statut, NULL, NULL, date_creation
                FROM chunks;

                DROP TABLE chunks;

                ALTER TABLE chunks_new RENAME TO chunks;

                PRAGMA foreign_keys = ON;
                "#,
            )?;
        }

        Ok(())
    }

    /// Serializes a 384-dimensional float vector into a 1,536-byte Little-Endian buffer.
    #[inline]
    pub fn serialize_vector(embedding: &[f32; 384]) -> [u8; 1536] {
        let mut bytes = [0u8; 1536];
        for (i, &val) in embedding.iter().enumerate() {
            let le = val.to_le_bytes();
            bytes[i * 4..(i + 1) * 4].copy_from_slice(&le);
        }
        bytes
    }

    /// Deserializes a 1,536-byte Little-Endian buffer back into a 384-dimensional float vector.
    #[inline]
    pub fn deserialize_vector(bytes: &[u8; 1536]) -> [f32; 384] {
        let mut embedding = [0.0f32; 384];
        for (i, chunk) in bytes.chunks_exact(4).enumerate() {
            let le: [u8; 4] = [chunk[0], chunk[1], chunk[2], chunk[3]];
            embedding[i] = f32::from_le_bytes(le);
        }
        embedding
    }

    /// Inserts or replaces a 384D embedding in `vec_chunks` associated with the relational `chunks.id`.
    pub fn insert_chunk_vector(&self, rowid: i64, embedding: &[f32; 384]) -> Result<()> {
        let bytes = Self::serialize_vector(embedding);
        self.conn.execute(
            "DELETE FROM vec_chunks WHERE rowid = ?1;",
            rusqlite::params![rowid],
        )?;
        self.conn.execute(
            "INSERT INTO vec_chunks(rowid, embedding) VALUES (?1, ?2);",
            rusqlite::params![rowid, &bytes[..]],
        )?;
        Ok(())
    }

    /// Queries `vec_chunks` for the `limit` nearest neighbors using cosine distance.
    /// Returns `(rowid, cosine_distance)` ordered by ascending distance.
    pub fn search_vector(
        &self,
        query_vector: &[f32; 384],
        limit: usize,
    ) -> Result<Vec<(i64, f32)>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let bytes = Self::serialize_vector(query_vector);
        let limit_i64 = i64::try_from(limit).unwrap_or(50);

        let mut stmt = self.conn.prepare(
            r#"
            SELECT rowid, distance
            FROM vec_chunks
            WHERE embedding MATCH ?1 AND k = ?2;
            "#,
        )?;

        let rows = stmt.query_map(rusqlite::params![&bytes[..], limit_i64], |row| {
            let rowid: i64 = row.get(0)?;
            let distance: f32 = row.get(1)?;
            Ok((rowid, distance))
        })?;

        let capacity = if limit_i64 > 0 {
            (limit_i64 as usize).min(1024)
        } else {
            0
        };
        let mut results = Vec::with_capacity(capacity);
        for row in rows {
            results.push(row?);
        }

        Ok(results)
    }

    /// Enregistre une relation dans `file_links`.
    pub fn insert_file_link(
        &self,
        source_path: &str,
        target_path: &str,
        link_type: &str,
        created_at: i64,
    ) -> Result<()> {
        // S'assure que source_path existe dans files pour respecter la clé étrangère
        self.conn.execute(
            r#"
            INSERT INTO files (file_path, file_hash, last_modified, frontmatter_json)
            VALUES (?1, '', ?2, NULL)
            ON CONFLICT(file_path) DO NOTHING;
            "#,
            rusqlite::params![source_path, created_at],
        )?;

        self.conn.execute(
            r#"
            INSERT INTO file_links (source_path, target_path, link_type, created_at)
            VALUES (?1, ?2, ?3, ?4);
            "#,
            rusqlite::params![source_path, target_path, link_type, created_at],
        )?;
        Ok(())
    }

    /// Résout le fichier remplaçant un document déprécié via traversée 1-hop du graphe.
    pub fn get_superseding_file(&self, target_path: &str) -> Result<Option<String>> {
        // 1. Si target_path est le document obsolète et un autre document source_path le remplace ('supersedes')
        let mut stmt = self.conn.prepare(
            "SELECT source_path FROM file_links WHERE target_path = ?1 AND link_type = 'supersedes' ORDER BY created_at DESC LIMIT 1;"
        )?;
        let mut rows = stmt.query(rusqlite::params![target_path])?;
        if let Some(row) = rows.next()? {
            let direct_successor: String = row.get(0)?;
            return Ok(Some(direct_successor));
        }

        // 2. Si source_path est le document déprécié pointant vers son remplaçant target_path
        let mut stmt = self.conn.prepare(
            "SELECT target_path FROM file_links WHERE source_path = ?1 AND link_type = 'superseded_by' ORDER BY created_at DESC LIMIT 1;"
        )?;
        let mut rows = stmt.query(rusqlite::params![target_path])?;
        if let Some(row) = rows.next()? {
            let direct_successor: String = row.get(0)?;
            return Ok(Some(direct_successor));
        }

        // 3. Repli : colonne superseded_by dans chunks
        let mut stmt = self.conn.prepare(
            "SELECT superseded_by FROM chunks WHERE file_path = ?1 AND superseded_by IS NOT NULL AND superseded_by != '' LIMIT 1;"
        )?;
        let mut rows = stmt.query(rusqlite::params![target_path])?;
        if let Some(row) = rows.next()? {
            let direct_successor: String = row.get(0)?;
            return Ok(Some(direct_successor));
        }

        Ok(None)
    }

    /// Retourne l'identifiant entier unique (rowid) d'un fragment par son chunk_id textuel.
    pub fn get_chunk_rowid(&self, chunk_id: &str) -> Result<Option<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM chunks WHERE chunk_id = ?1;")?;
        let mut rows = stmt.query(rusqlite::params![chunk_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// Indexe un fragment de note (chunk) dans la table `chunks` et retourne son rowid 64-bit.
    pub fn index_chunk(&self, chunk: &IndexedChunk) -> Result<i64> {
        let coala_type_str = chunk.coala_type_str();
        let status_str = chunk.status_str();

        let rowid: i64 = self.conn.query_row(
            r#"
            INSERT INTO chunks (
                chunk_id, file_path, chunk_index, content,
                token_count, coala_type, status, superseded_by, deprecated_at, date_creation
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(chunk_id) DO UPDATE SET
                file_path = excluded.file_path,
                chunk_index = excluded.chunk_index,
                content = excluded.content,
                token_count = excluded.token_count,
                coala_type = excluded.coala_type,
                status = excluded.status,
                superseded_by = excluded.superseded_by,
                deprecated_at = excluded.deprecated_at,
                date_creation = excluded.date_creation
            RETURNING id;
            "#,
            rusqlite::params![
                chunk.chunk_id,
                chunk.file_path,
                chunk.chunk_index as i64,
                chunk.content,
                chunk.token_count as i64,
                coala_type_str,
                status_str,
                chunk.superseded_by.as_deref(),
                chunk.deprecated_at,
                chunk.date_creation,
            ],
            |row| row.get(0),
        )?;

        Ok(rowid)
    }

    /// Supprime un fragment par son identifiant unique textuel.
    pub fn delete_chunk(&self, chunk_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM chunks WHERE chunk_id = ?1;",
            rusqlite::params![chunk_id],
        )?;
        Ok(())
    }

    /// Enregistre ou met à jour les métadonnées d'un fichier dans la table `files`.
    pub fn upsert_file(
        &self,
        file_path: &str,
        file_hash: &str,
        last_modified: i64,
        frontmatter_json: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            r#"
            INSERT INTO files (file_path, file_hash, last_modified, frontmatter_json)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(file_path) DO UPDATE SET
                file_hash = excluded.file_hash,
                last_modified = excluded.last_modified,
                frontmatter_json = excluded.frontmatter_json;
            "#,
            rusqlite::params![file_path, file_hash, last_modified, frontmatter_json],
        )?;
        Ok(())
    }

    /// Supprime un fichier et propage la suppression en cascade sur ses fragments, liens et vecteurs.
    pub fn delete_file(&self, file_path: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM files WHERE file_path = ?1;",
            rusqlite::params![file_path],
        )?;
        // Nettoyage défensif explicite des vecteurs orphelins dans vec_chunks
        self.conn.execute(
            "DELETE FROM vec_chunks WHERE rowid NOT IN (SELECT id FROM chunks);",
            [],
        )?;
        Ok(())
    }

    /// Effectue une recherche plein texte BM25 sur la table virtuelle `fts_notes`.
    pub fn search_fts(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        let limit_i64 = i64::try_from(limit).unwrap_or(50);
        let query_str = sanitize_fts5_query(trimmed);
        if query_str.is_empty() {
            return Ok(Vec::new());
        }

        let hl_start = "\u{E000}";
        let hl_end = "\u{E001}";

        let sql = format!(
            r#"
            SELECT
                f.chunk_id,
                c.file_path,
                snippet(fts_notes, 1, '{hl_start}', '{hl_end}', '...', 32) AS snippet,
                -bm25(fts_notes) AS score,
                c.status AS statut,
                c.date_creation,
                fi.frontmatter_json
            FROM fts_notes f
            JOIN chunks c ON c.chunk_id = f.chunk_id
            LEFT JOIN files fi ON fi.file_path = c.file_path
            WHERE fts_notes MATCH ?1
            ORDER BY score DESC
            LIMIT ?2
            "#
        );

        let mut stmt = self.conn.prepare(&sql)?;

        let mut execute_search = |q: &str| -> rusqlite::Result<Vec<SearchResult>> {
            let rows = stmt.query_map(rusqlite::params![q, limit_i64], |row| {
                let chunk_id: String = row.get(0)?;
                let file_path: String = row.get(1)?;
                let raw_snippet: String = row.get(2)?;
                let score: f64 = row.get(3)?;
                let statut: String = row.get(4)?;
                let date_creation_num: i64 = row.get(5)?;
                let frontmatter_json: Option<String> = row.get(6)?;

                // Échappement HTML strict contre les attaques XSS
                let snippet = html_escape(&raw_snippet)
                    .replace(hl_start, "<mark>")
                    .replace(hl_end, "</mark>");

                let title = frontmatter_json
                    .as_deref()
                    .and_then(|json_str| serde_json::from_str::<serde_json::Value>(json_str).ok())
                    .and_then(|v| {
                        v.get("title")
                            .and_then(|t| t.as_str())
                            .map(ToString::to_string)
                    })
                    .unwrap_or_else(|| {
                        Path::new(&file_path)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or(&file_path)
                            .to_string()
                    });

                Ok(SearchResult {
                    chunk_id,
                    file_path,
                    title,
                    snippet,
                    score,
                    statut,
                    date_creation: date_creation_num.to_string(),
                })
            })?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row?);
            }
            Ok(results)
        };

        let results = match execute_search(&query_str) {
            Ok(res) if !res.is_empty() => res,
            Ok(_) => {
                tracing::debug!(
                    "[Storage] [REPLI] Requête FTS5 standard '{}' sans résultat. Repli sur la syntaxe préfixe.",
                    query_str
                );
                let fallback = query_str
                    .split_whitespace()
                    .map(|w| format!("\"{}\"*", w.replace('"', "")))
                    .collect::<Vec<_>>()
                    .join(" ");
                execute_search(&fallback).unwrap_or_default()
            }
            Err(err) => {
                tracing::warn!(
                    "[Storage] [REPLI] Erreur syntaxe FTS5 pour '{}' ({:?}). Repli automatique sur la syntaxe préfixe.",
                    query_str,
                    err
                );
                let fallback = query_str
                    .split_whitespace()
                    .map(|w| format!("\"{}\"*", w.replace('"', "")))
                    .collect::<Vec<_>>()
                    .join(" ");
                execute_search(&fallback).unwrap_or_default()
            }
        };

        if results.is_empty() {
            let keywords = extract_search_keywords(trimmed);
            if !keywords.is_empty() {
                tracing::debug!(
                    "[Storage] [REPLI] Recherche FTS5 initiale et préfixe sans résultat. Repli en cascade sur l'extraction de mots-clés : {:?}",
                    keywords
                );
                // Essai 1 : tous les mots-clés en AND
                let and_query = keywords
                    .iter()
                    .map(|k| format!("\"{}\"", k.replace('"', "")))
                    .collect::<Vec<_>>()
                    .join(" ");
                if let Ok(res) = execute_search(&and_query) {
                    if !res.is_empty() {
                        return Ok(res);
                    }
                }

                // Essai 2 : mots-clés en OR (classement BM25)
                let or_query = keywords
                    .iter()
                    .map(|k| format!("\"{}\"", k.replace('"', "")))
                    .collect::<Vec<_>>()
                    .join(" OR ");
                if let Ok(res) = execute_search(&or_query) {
                    if !res.is_empty() {
                        return Ok(res);
                    }
                }

                // Essai 3 : préfixe en OR (tolérance flexions/accords)
                let prefix_or = keywords
                    .iter()
                    .map(|k| format!("\"{}\"*", k.replace('"', "")))
                    .collect::<Vec<_>>()
                    .join(" OR ");
                if let Ok(res) = execute_search(&prefix_or) {
                    if !res.is_empty() {
                        return Ok(res);
                    }
                }
            }
        }

        Ok(results)
    }

    /// Recherche tolérante adaptée aux questions en langage naturel pour le RAG.
    pub fn search_question(&self, question: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let keywords = extract_search_keywords(question);
        if !keywords.is_empty() {
            let fts_query = keywords
                .iter()
                .map(|k| format!("\"{}\"*", k.replace('"', "")))
                .collect::<Vec<_>>()
                .join(" OR ");

            let res = self.search_fts(&fts_query, limit)?;
            if !res.is_empty() {
                return Ok(res);
            }
        }

        self.search_fts(question, limit)
    }

    /// Calcule les statistiques d'indexation du coffre.
    pub fn get_stats(&self) -> Result<VaultStats> {
        let total_files: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
        let total_chunks: usize =
            self.conn
                .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))?;
        let last_scan_timestamp: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(last_modified), 0) FROM files",
            [],
            |row| row.get(0),
        )?;

        Ok(VaultStats {
            total_files,
            total_chunks,
            last_scan_timestamp,
        })
    }

    /// Retourne une référence vers la connexion SQLite sous-jacente (pour inspection et tests).
    pub fn raw_connection(&self) -> &Connection {
        &self.conn
    }
}

/// Extrait les mots-clés significatifs d'un texte ou d'une question en filtrant la ponctuation et les mots vides.
pub fn extract_search_keywords(text: &str) -> Vec<String> {
    let clean = text
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>();

    let stop_words: std::collections::HashSet<&'static str> = [
        // Français
        "de", "des", "du", "le", "la", "les", "un", "une", "est", "sont", "été", "ete", "être",
        "etre", "a", "ont", "avoir", "ce", "cet", "cette", "ces", "mon", "ma", "mes", "ton", "ta",
        "tes", "son", "sa", "ses", "notre", "nos", "votre", "vos", "leur", "leurs", "qui", "que",
        "quoi", "dont", "ou", "où", "quand", "comment", "pourquoi", "quel", "quelle", "quels",
        "quelles", "combien", "dans", "sur", "sous", "pour", "par", "avec", "sans", "en", "au",
        "aux", "y", "il", "elle", "on", "ils", "elles", "je", "tu", "nous", "vous", "me", "te",
        "se", "moi", "toi", "lui", "eux", "ceci", "cela", "ca", "ça", "quelque", "quelques",
        "plus", "moins", "tres", "très", "bien", "faire", "fait", // Anglais
        "what", "which", "who", "whom", "where", "when", "why", "how", "is", "are", "was", "were",
        "be", "been", "the", "in", "on", "at", "to", "for", "of", "with", "my", "your", "his",
        "her", "their", "our",
    ]
    .into_iter()
    .collect();

    let mut keywords = Vec::new();
    for word in clean.split_whitespace() {
        let lower = word.to_lowercase();
        if lower.len() >= 2 && !stop_words.contains(lower.as_str()) {
            keywords.push(word.to_string());
        }
    }

    if keywords.is_empty() {
        for word in clean.split_whitespace() {
            if word.len() >= 2 {
                keywords.push(word.to_string());
            }
        }
    }

    keywords
}

fn sanitize_fts5_query(query: &str) -> String {
    let trimmed = query.trim();
    let quote_count = trimmed.chars().filter(|c| *c == '"').count();
    if quote_count % 2 != 0 {
        trimmed.replace('"', "")
    } else {
        trimmed.to_string()
    }
}

fn html_escape(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            _ => output.push(c),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_question_keyword_fallback() {
        let storage = StorageManager::open_in_memory().unwrap();
        storage.init_schema().unwrap();

        storage
            .raw_connection()
            .execute(
                "INSERT INTO files (file_path, file_hash, last_modified, frontmatter_json) VALUES (?1, ?2, ?3, ?4)",
                ("test.md", "hash_test", 1710000000i64, "{}"),
            )
            .unwrap();

        let chunk = crate::models::IndexedChunk::new(
            "test:0",
            "test.md",
            0,
            "Implémentation du réseau neural et apprentissage automatique",
            5,
            crate::models::CoalaType::Semantic,
            crate::models::NoteStatus::Active,
            0,
        );
        storage.index_chunk(&chunk).unwrap();

        let res = storage
            .search_question("Comment configurer le réseau ?", 5)
            .unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].file_path, "test.md");
    }
}
