use crate::error::Result;
use crate::models::{IndexedChunk, SearchResult, VaultStats};
use rusqlite::Connection;
use std::path::Path;

/// Gestionnaire de persistance locale SQLite et index FTS5.
pub struct StorageManager {
    conn: Connection,
}

impl StorageManager {
    /// Ouvre ou crée une base de données SQLite au chemin spécifié.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        if let Some(parent) = path_ref.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(path_ref)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        Ok(Self { conn })
    }

    /// Ouvre une base de données en mémoire vive (pour les tests rapides).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        Ok(Self { conn })
    }

    /// Initialise la configuration PRAGMA (WAL, foreign_keys, etc.) et le schéma (tables, FTS5, triggers).
    pub fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            PRAGMA busy_timeout = 5000;

            CREATE TABLE IF NOT EXISTS files (
                file_path TEXT PRIMARY KEY,
                file_hash TEXT NOT NULL,
                last_modified INTEGER NOT NULL,
                frontmatter_json TEXT
            );

            CREATE TABLE IF NOT EXISTS chunks (
                chunk_id TEXT PRIMARY KEY,
                file_path TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                content TEXT NOT NULL,
                token_count INTEGER NOT NULL,
                note_type TEXT NOT NULL,
                statut TEXT NOT NULL,
                date_creation INTEGER NOT NULL,
                FOREIGN KEY(file_path) REFERENCES files(file_path) ON DELETE CASCADE
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS fts_notes USING fts5(
                chunk_id UNINDEXED,
                content,
                file_path UNINDEXED,
                tokenize = 'porter unicode61'
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
            "#,
        )?;
        Ok(())
    }

    /// Indexe un fragment de note (chunk) dans la table `chunks`.
    pub fn index_chunk(&self, chunk: &IndexedChunk) -> Result<()> {
        self.conn.execute(
            r#"
            INSERT INTO chunks (
                chunk_id, file_path, chunk_index, content,
                token_count, note_type, statut, date_creation
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(chunk_id) DO UPDATE SET
                file_path = excluded.file_path,
                chunk_index = excluded.chunk_index,
                content = excluded.content,
                token_count = excluded.token_count,
                note_type = excluded.note_type,
                statut = excluded.statut,
                date_creation = excluded.date_creation;
            "#,
            rusqlite::params![
                chunk.chunk_id,
                chunk.file_path,
                chunk.chunk_index as i64,
                chunk.content,
                chunk.token_count as i64,
                chunk.note_type,
                chunk.statut,
                chunk.date_creation,
            ],
        )?;
        Ok(())
    }

    /// Supprime un fragment par son identifiant unique.
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

    /// Supprime un fichier et propage la suppression en cascade sur ses fragments.
    pub fn delete_file(&self, file_path: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM files WHERE file_path = ?1;",
            rusqlite::params![file_path],
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
                c.statut,
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

        match execute_search(&query_str) {
            Ok(results) => Ok(results),
            Err(_) => {
                // Repli sécurisé en cas d'erreur de syntaxe FTS5 (ex. guillemets ou opérateurs)
                let fallback = query_str
                    .split_whitespace()
                    .map(|w| format!("\"{}\"*", w.replace('"', "")))
                    .collect::<Vec<_>>()
                    .join(" ");
                if let Ok(results) = execute_search(&fallback) {
                    Ok(results)
                } else {
                    Ok(Vec::new())
                }
            }
        }
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
