//! Suite de productivité locale et manipulation de fichiers Markdown "File-over-App".
//! Fonctions utilitaires rapides pour tâches, journalisation, réunions, signets et calculs.

use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Élément de tâche extrait d'une note Markdown.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskItem {
    pub file_path: String,
    pub line_number: usize,
    pub content: String,
    pub checked: bool,
    pub created_at: Option<String>,
}

/// Modèle réutilisable de texte ou snippet.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnippetItem {
    pub key: String,
    pub title: String,
    pub content: String,
}

/// Résultat d'une évaluation arithmétique.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MathEvaluationResult {
    pub expression: String,
    pub result: f64,
    pub formatted: String,
}

/// Nettoie les artefacts d'imprécision binaire IEEE-754 des nombres flottants.
/// Exemples :
/// - 1.2 * 56.4 = 67.67999999999999 -> 67.68
/// - 0.1 + 0.2 = 0.30000000000000004 -> 0.3
pub fn sanitize_float_precision(val: f64) -> f64 {
    if val == 0.0 || !val.is_finite() {
        return val;
    }
    let mag = val.abs().log10().floor() as i32;
    if !(-15..=15).contains(&mag) {
        let formatted = format!("{:.12e}", val);
        return formatted.parse::<f64>().unwrap_or(val);
    }
    let decimals = (12 - 1 - mag).clamp(0, 18) as usize;
    let formatted = format!("{:.decimals$}", val, decimals = decimals);
    formatted.parse::<f64>().unwrap_or(val)
}

/// Formate un résultat arithmétique en chaîne sans artefact flottant.
pub fn format_math_result(val: f64) -> String {
    let sanitized = sanitize_float_precision(val);
    format!("{sanitized}")
}

/// Évalue une expression mathématique et retourne un `MathEvaluationResult` structuré.
pub fn evaluate_math_detailed(input: &str) -> Result<MathEvaluationResult, String> {
    let result = evaluate_math_expression(input)?;
    let formatted = format_math_result(result);
    Ok(MathEvaluationResult {
        expression: input.trim().to_string(),
        result,
        formatted,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Évaluateur Arithmétique Déterministe et Sécurisé (Recursive Descent)
// ─────────────────────────────────────────────────────────────────────────────

/// Évalue une expression mathématique simple (+, -, *, /, ^, %, parenthèses).
/// Retourne une erreur explicite si l'expression n'est pas arithmétique ou divise par zéro.
pub fn evaluate_math_expression(input: &str) -> Result<f64, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Expression vide".to_string());
    }

    let tokens = tokenize_math(trimmed)?;
    if tokens.is_empty() {
        return Err("Aucun opérateur ou nombre détecté".to_string());
    }

    let mut pos = 0;
    let res = parse_expression(&tokens, &mut pos)?;

    if pos < tokens.len() {
        return Err("Caractères inattendus en fin d'expression".to_string());
    }

    if res.is_nan() || res.is_infinite() {
        return Err("Résultat mathématique indéfini ou infini".to_string());
    }

    Ok(sanitize_float_precision(res))
}

#[derive(Debug, PartialEq, Clone)]
enum Token {
    Number(f64),
    Plus,
    Minus,
    Multiply,
    Divide,
    Modulo,
    Power,
    LParen,
    RParen,
}

fn tokenize_math(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }

        match c {
            '+' => tokens.push(Token::Plus),
            '-' => tokens.push(Token::Minus),
            '*' => tokens.push(Token::Multiply),
            '/' => tokens.push(Token::Divide),
            '%' => tokens.push(Token::Modulo),
            '^' => tokens.push(Token::Power),
            '(' => tokens.push(Token::LParen),
            ')' => tokens.push(Token::RParen),
            '0'..='9' | '.' | ',' => {
                let mut num_str = String::new();
                let mut has_dot = false;
                while i < chars.len()
                    && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == ',')
                {
                    if chars[i] == '.' || chars[i] == ',' {
                        if has_dot {
                            return Err(
                                "Nombre décimal invalide avec multiples points ou virgules"
                                    .to_string(),
                            );
                        }
                        has_dot = true;
                        num_str.push('.');
                    } else {
                        num_str.push(chars[i]);
                    }
                    i += 1;
                }
                let val = num_str
                    .parse::<f64>()
                    .map_err(|e| format!("Nombre invalide : {e}"))?;
                tokens.push(Token::Number(sanitize_float_precision(val)));
                continue;
            }
            _ => return Err(format!("Caractère non arithmétique : '{c}'")),
        }
        i += 1;
    }

    Ok(tokens)
}

fn parse_expression(tokens: &[Token], pos: &mut usize) -> Result<f64, String> {
    let mut val = parse_term(tokens, pos)?;

    while *pos < tokens.len() {
        match tokens[*pos] {
            Token::Plus => {
                *pos += 1;
                val = sanitize_float_precision(val + parse_term(tokens, pos)?);
            }
            Token::Minus => {
                *pos += 1;
                val = sanitize_float_precision(val - parse_term(tokens, pos)?);
            }
            _ => break,
        }
    }
    Ok(val)
}

fn parse_term(tokens: &[Token], pos: &mut usize) -> Result<f64, String> {
    let mut val = parse_power(tokens, pos)?;

    while *pos < tokens.len() {
        match tokens[*pos] {
            Token::Multiply => {
                *pos += 1;
                val = sanitize_float_precision(val * parse_power(tokens, pos)?);
            }
            Token::Divide => {
                *pos += 1;
                let denom = parse_power(tokens, pos)?;
                if denom.abs() < f64::EPSILON {
                    return Err("Division par zéro".to_string());
                }
                val = sanitize_float_precision(val / denom);
            }
            Token::Modulo => {
                *pos += 1;
                let denom = parse_power(tokens, pos)?;
                if denom.abs() < f64::EPSILON {
                    return Err("Modulo par zéro".to_string());
                }
                val = sanitize_float_precision(val % denom);
            }
            _ => break,
        }
    }
    Ok(val)
}

fn parse_power(tokens: &[Token], pos: &mut usize) -> Result<f64, String> {
    let base = parse_factor(tokens, pos)?;

    if *pos < tokens.len() && tokens[*pos] == Token::Power {
        *pos += 1;
        let exp = parse_power(tokens, pos)?;
        Ok(sanitize_float_precision(base.powf(exp)))
    } else {
        Ok(base)
    }
}

fn parse_factor(tokens: &[Token], pos: &mut usize) -> Result<f64, String> {
    if *pos >= tokens.len() {
        return Err("Fin d'expression inattendue".to_string());
    }

    match tokens[*pos] {
        Token::Number(n) => {
            *pos += 1;
            Ok(n)
        }
        Token::Plus => {
            *pos += 1;
            parse_factor(tokens, pos)
        }
        Token::Minus => {
            *pos += 1;
            let val = parse_factor(tokens, pos)?;
            Ok(-val)
        }
        Token::LParen => {
            *pos += 1;
            let val = parse_expression(tokens, pos)?;
            if *pos >= tokens.len() || tokens[*pos] != Token::RParen {
                return Err("Parenthèse fermante ')' manquante".to_string());
            }
            *pos += 1;
            Ok(val)
        }
        _ => Err("Facteur mathématique attendu".to_string()),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Gestion des Tâches (Inbox.md & Notes du coffre)
// ─────────────────────────────────────────────────────────────────────────────

/// Ajoute une tâche `- [ ]` dans `Inbox.md`.
pub fn append_todo(vault_path: &Path, content: &str) -> Result<String, String> {
    let clean_text = content.trim();
    if clean_text.is_empty() {
        return Err("Le contenu de la tâche ne peut pas être vide".to_string());
    }

    let inbox_file = vault_path.join("Inbox.md");
    let now = Local::now();
    let time_str = now.format("%H:%M").to_string();
    let now_iso = now.to_rfc3339();

    if !inbox_file.exists() {
        let initial = format!(
            "---\nid: inbox\ntitle: Boîte de Réception\ndate_creation: \"{now_iso}\"\ndate_modification: \"{now_iso}\"\nnote_type: episodique\nstatut: actif\ntags:\n  - inbox\n  - todo\n---\n\n# Boîte de Réception (Inbox)\n\n## Tâches à traiter\n- [ ] [{time_str}] {clean_text}\n"
        );
        fs::write(&inbox_file, initial)
            .map_err(|e| format!("Impossible d'écrire Inbox.md : {e}"))?;
    } else {
        let existing = fs::read_to_string(&inbox_file)
            .map_err(|e| format!("Impossible de lire Inbox.md : {e}"))?;
        let entry = format!("- [ ] [{time_str}] {clean_text}\n");
        let updated = format!("{}{entry}", ensure_trailing_newline(&existing));
        fs::write(&inbox_file, updated)
            .map_err(|e| format!("Impossible d'écrire Inbox.md : {e}"))?;
    }

    Ok(inbox_file.to_string_lossy().to_string())
}

/// Extrait toutes les cases à cocher (`- [ ]` ou `- [x]`) d'un fichier Markdown.
pub fn extract_tasks_from_file(file_path: &Path) -> Result<Vec<TaskItem>, String> {
    let content = fs::read_to_string(file_path)
        .map_err(|e| format!("Impossible de lire {}: {e}", file_path.display()))?;

    let mut tasks = Vec::new();
    let file_path_str = file_path.to_string_lossy().to_string();

    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
            tasks.push(TaskItem {
                file_path: file_path_str.clone(),
                line_number: idx + 1,
                content: rest.trim().to_string(),
                checked: false,
                created_at: None,
            });
        } else if let Some(rest) = trimmed
            .strip_prefix("- [x] ")
            .or_else(|| trimmed.strip_prefix("- [X] "))
        {
            tasks.push(TaskItem {
                file_path: file_path_str.clone(),
                line_number: idx + 1,
                content: rest.trim().to_string(),
                checked: true,
                created_at: None,
            });
        }
    }

    Ok(tasks)
}

/// Modifie l'état d'une tâche à une ligne donnée dans un fichier Markdown.
pub fn toggle_task_in_file(
    file_path: &Path,
    line_number: usize,
    checked: bool,
) -> Result<(), String> {
    let content = fs::read_to_string(file_path)
        .map_err(|e| format!("Impossible de lire {}: {e}", file_path.display()))?;

    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    if line_number == 0 || line_number > lines.len() {
        return Err(format!("Numéro de ligne {line_number} hors limites"));
    }

    let target_idx = line_number - 1;
    let old_line = &lines[target_idx];
    let new_line = if checked {
        if old_line.contains("- [ ] ") {
            old_line.replace("- [ ] ", "- [x] ")
        } else {
            old_line.clone()
        }
    } else if old_line.contains("- [x] ") {
        old_line.replace("- [x] ", "- [ ] ")
    } else if old_line.contains("- [X] ") {
        old_line.replace("- [X] ", "- [ ] ")
    } else {
        old_line.clone()
    };

    lines[target_idx] = new_line;
    let updated_content = lines.join("\n") + "\n";
    fs::write(file_path, updated_content)
        .map_err(|e| format!("Impossible d'écrire {}: {e}", file_path.display()))?;

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Journal & Micro-Journaling (`/log`)
// ─────────────────────────────────────────────────────────────────────────────

/// Ajoute une entrée horodatée dans la note du jour `Journal/YYYY-MM-DD.md`.
pub fn append_log_entry(vault_path: &Path, content: &str) -> Result<String, String> {
    let clean_text = content.trim();
    if clean_text.is_empty() {
        return Err("Le contenu du journal ne peut pas être vide".to_string());
    }

    let journal_dir = vault_path.join("Journal");
    if !journal_dir.exists() {
        fs::create_dir_all(&journal_dir)
            .map_err(|e| format!("Impossible de créer Journal/ : {e}"))?;
    }

    let now = Local::now();
    let date_str = now.format("%Y-%m-%d").to_string();
    let time_str = now.format("%H:%M").to_string();
    let now_iso = now.to_rfc3339();
    let note_file = journal_dir.join(format!("{date_str}.md"));

    if !note_file.exists() {
        let initial = format!(
            "---\nid: journal-{date_str}\ntitle: Journal {date_str}\ndate_creation: \"{now_iso}\"\ndate_modification: \"{now_iso}\"\nnote_type: episodique\nstatut: actif\ntags:\n  - journal\n---\n\n# Journal - {date_str}\n\n## Journal de bord\n- **{time_str}** : {clean_text}\n"
        );
        fs::write(&note_file, initial)
            .map_err(|e| format!("Impossible d'écrire la note journal : {e}"))?;
    } else {
        let existing = fs::read_to_string(&note_file)
            .map_err(|e| format!("Impossible de lire la note journal : {e}"))?;
        let entry = format!("- **{time_str}** : {clean_text}\n");
        let updated = format!("{}{entry}", ensure_trailing_newline(&existing));
        fs::write(&note_file, updated)
            .map_err(|e| format!("Impossible de mettre à jour le journal : {e}"))?;
    }

    Ok(note_file.to_string_lossy().to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Modèle de Réunion (`/meeting`)
// ─────────────────────────────────────────────────────────────────────────────

/// Crée une fiche de réunion structurée dans `Reunions/YYYY-MM-DD - [Titre].md`.
pub fn create_meeting_note(vault_path: &Path, title: &str) -> Result<String, String> {
    let clean_title = title.trim();
    if clean_title.is_empty() {
        return Err("Le titre de la réunion ne peut pas être vide".to_string());
    }

    let safe_title = clean_title.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "-");
    let meetings_dir = vault_path.join("Reunions");
    if !meetings_dir.exists() {
        fs::create_dir_all(&meetings_dir)
            .map_err(|e| format!("Impossible de créer Reunions/ : {e}"))?;
    }

    let now = Local::now();
    let date_str = now.format("%Y-%m-%d").to_string();
    let time_str = now.format("%H:%M").to_string();
    let now_iso = now.to_rfc3339();
    let note_file = meetings_dir.join(format!("{date_str} - {safe_title}.md"));

    let template = format!(
        r#"---
id: reunion-{date_str}-{safe_title}
title: "Réunion : {clean_title}"
date_creation: "{now_iso}"
date_modification: "{now_iso}"
note_type: episodique
statut: actif
tags:
  - reunion
  - compte-rendu
---

# Réunion : {clean_title}

## Informations
- **Date** : {date_str}
- **Heure** : {time_str}

## Participants
- Moi
- 

## Ordre du jour
1. Point de situation
2. Arbitrages et décisions
3. Prochaines étapes

## Notes de séance


## Actions à mener
- [ ] 
"#
    );

    fs::write(&note_file, template)
        .map_err(|e| format!("Impossible d'écrire la note de réunion : {e}"))?;

    Ok(note_file.to_string_lossy().to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Signets & Snippets (`/bookmark`, `/snip`)
// ─────────────────────────────────────────────────────────────────────────────

/// Ajoute un signet web dans `Ressources/Bookmarks.md`.
pub fn append_bookmark(
    vault_path: &Path,
    url: &str,
    comment: Option<String>,
) -> Result<String, String> {
    let clean_url = url.trim();
    if clean_url.is_empty() {
        return Err("L'URL du signet ne peut pas être vide".to_string());
    }

    let res_dir = vault_path.join("Ressources");
    if !res_dir.exists() {
        fs::create_dir_all(&res_dir)
            .map_err(|e| format!("Impossible de créer Ressources/ : {e}"))?;
    }

    let now = Local::now();
    let date_str = now.format("%Y-%m-%d %H:%M").to_string();
    let now_iso = now.to_rfc3339();
    let bookmark_file = res_dir.join("Bookmarks.md");

    let comment_str = comment.map(|c| format!(" — {c}")).unwrap_or_default();
    let entry = format!("- [ ] [{clean_url}]({clean_url}){comment_str} *(Ajouté le {date_str})*\n");

    if !bookmark_file.exists() {
        let initial = format!(
            "---\nid: bookmarks\ntitle: Signets & Liens Utiles\ndate_creation: \"{now_iso}\"\ndate_modification: \"{now_iso}\"\nnote_type: semantique\nstatut: actif\ntags:\n  - ressources\n  - signets\n---\n\n# Signets & Liens Utiles\n\n## Liens sauvegardés\n{entry}"
        );
        fs::write(&bookmark_file, initial)
            .map_err(|e| format!("Impossible d'écrire Bookmarks.md : {e}"))?;
    } else {
        let existing = fs::read_to_string(&bookmark_file)
            .map_err(|e| format!("Impossible de lire Bookmarks.md : {e}"))?;
        let updated = format!("{}{entry}", ensure_trailing_newline(&existing));
        fs::write(&bookmark_file, updated)
            .map_err(|e| format!("Impossible de mettre à jour Bookmarks.md : {e}"))?;
    }

    Ok(bookmark_file.to_string_lossy().to_string())
}

/// Retourne la liste des snippets de texte disponibles.
/// Lit `Templates/Snippets.md` si présent, ou fournit les snippets standards du système.
pub fn load_snippets(vault_path: &Path) -> Vec<SnippetItem> {
    let mut snippets = vec![
        SnippetItem {
            key: "mail-pro".to_string(),
            title: "Email Professionnel Formel".to_string(),
            content: "Bonjour,\n\nJe vous remercie pour votre retour. Je reviens vers vous dès que possible avec les éléments demandés.\n\nBien cordialement,\n".to_string(),
        },
        SnippetItem {
            key: "reunion-rapide".to_string(),
            title: "Trame Réunion Rapide".to_string(),
            content: "### Sujet :\n- **Objectif** :\n- **Décisions** :\n- **Next steps** :\n".to_string(),
        },
        SnippetItem {
            key: "todo-p1".to_string(),
            title: "Tâche Prioritaire [P1]".to_string(),
            content: "- [ ] 🚨 **[P1]** ".to_string(),
        },
        SnippetItem {
            key: "code-block".to_string(),
            title: "Bloc de code Markdown".to_string(),
            content: "```rust\n// TODO: implémenter\n```\n".to_string(),
        },
    ];

    let custom_file = vault_path.join("Templates").join("Snippets.md");
    if let Ok(content) = fs::read_to_string(custom_file) {
        for block in content.split("---") {
            let lines: Vec<&str> = block.trim().lines().collect();
            if lines.len() >= 2 && lines[0].starts_with('#') {
                let title = lines[0].trim_start_matches('#').trim().to_string();
                let body = lines[1..].join("\n").trim().to_string();
                let key = title.to_lowercase().replace(' ', "-");
                snippets.push(SnippetItem {
                    key,
                    title,
                    content: body,
                });
            }
        }
    }

    snippets
}

fn ensure_trailing_newline(text: &str) -> String {
    if text.ends_with('\n') {
        text.to_string()
    } else {
        format!("{text}\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_float_sanitization_user_case() {
        // Cas exact signalé par l'utilisateur
        if let Ok(res) = evaluate_math_expression("1.2*56.4") {
            assert_eq!(res, 67.68);
            assert_eq!(format_math_result(res), "67.68");
        } else {
            panic!("Should evaluate 1.2*56.4");
        }

        // Cas avec virgule
        if let Ok(res_comma) = evaluate_math_expression("1,2*56,4") {
            assert_eq!(res_comma, 67.68);
        } else {
            panic!("Should evaluate 1,2*56,4");
        }

        // Évaluation détaillée
        if let Ok(detailed) = evaluate_math_detailed("1.2 * 56.4") {
            assert_eq!(detailed.result, 67.68);
            assert_eq!(detailed.formatted, "67.68");
            assert_eq!(detailed.expression, "1.2 * 56.4");
        } else {
            panic!("Should evaluate detailed 1.2 * 56.4");
        }
    }

    #[test]
    fn test_float_sanitization_edge_cases() {
        assert_eq!(evaluate_math_expression("0.1 + 0.2").ok(), Some(0.3));
        assert_eq!(evaluate_math_expression("0.3 - 0.1").ok(), Some(0.2));
        assert_eq!(evaluate_math_expression("1.15 * 100").ok(), Some(115.0));
        assert_eq!(evaluate_math_expression("35.7 * 100").ok(), Some(3570.0));
        assert_eq!(
            evaluate_math_expression("1.2 * 56.4 - 67.68").ok(),
            Some(0.0)
        );
        assert_eq!(evaluate_math_expression("9 ^ 0.5").ok(), Some(3.0));
    }
}
