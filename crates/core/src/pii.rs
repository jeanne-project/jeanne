use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

fn get_email_regex() -> &'static Regex {
    static RE: LazyLock<Regex> =
        LazyLock::new(
            || match Regex::new(r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b") {
                Ok(r) => r,
                Err(_) => match Regex::new("^$") {
                    Ok(fallback) => fallback,
                    Err(_) => unreachable!(),
                },
            },
        );
    &RE
}

fn get_phone_regex() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        match Regex::new(r"\b(?:\+?\d{1,3}[-.\s]?)?\(?\d{2,4}\)?[-.\s]?\d{2,4}[-.\s]?\d{2,4}\b") {
            Ok(r) => r,
            Err(_) => match Regex::new("^$") {
                Ok(fallback) => fallback,
                Err(_) => unreachable!(),
            },
        }
    });
    &RE
}

fn get_financial_regex() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        match Regex::new(
            r"\b(?:[A-Z]{2}\d{2}[A-Z0-9]{11,30}|\d{4}[-\s]?\d{4}[-\s]?\d{4}[-\s]?\d{4})\b",
        ) {
            Ok(r) => r,
            Err(_) => match Regex::new("^$") {
                Ok(fallback) => fallback,
                Err(_) => unreachable!(),
            },
        }
    });
    &RE
}

#[derive(Debug, Default, Clone)]
pub struct PiiSession {
    pub forward_map: HashMap<String, String>,
    pub reverse_map: HashMap<String, String>,
    pub counter_email: usize,
    pub counter_phone: usize,
    pub counter_financial: usize,
}

impl PiiSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// Masque les données PII (emails, téléphones, identifiants financiers)
    /// de façon déterministe avec des balises ordonnées ([EMAIL_N], [PHONE_N], [FINANCIAL_N]).
    pub fn mask_text(&mut self, text: &str) -> String {
        let mut result = text.to_string();

        // 1. Masquage des Emails
        result = self.mask_pattern(&result, get_email_regex(), "EMAIL", |s| {
            s.counter_email += 1;
            s.counter_email
        });

        // 2. Masquage des Données Financières (IBAN / Cartes de crédit)
        result = self.mask_pattern(&result, get_financial_regex(), "FINANCIAL", |s| {
            s.counter_financial += 1;
            s.counter_financial
        });

        // 3. Masquage des Numéros de Téléphone
        result = self.mask_pattern(&result, get_phone_regex(), "PHONE", |s| {
            s.counter_phone += 1;
            s.counter_phone
        });

        result
    }

    fn mask_pattern<F>(
        &mut self,
        text: &str,
        regex: &Regex,
        prefix: &str,
        mut increment: F,
    ) -> String
    where
        F: FnMut(&mut Self) -> usize,
    {
        let mut new_text = String::with_capacity(text.len());
        let mut last_idx = 0;

        for m in regex.find_iter(text) {
            let matched_str = m.as_str();

            // Ne pas re-masquer un placeholder déjà généré (ex: [EMAIL_1])
            if matched_str.starts_with('[') && matched_str.ends_with(']') {
                continue;
            }

            new_text.push_str(&text[last_idx..m.start()]);

            let mask = match self.forward_map.get(matched_str) {
                Some(existing) => existing.clone(),
                None => {
                    let idx = increment(self);
                    let created = format!("[{prefix}_{idx}]");
                    self.forward_map
                        .insert(matched_str.to_string(), created.clone());
                    self.reverse_map
                        .insert(created.clone(), matched_str.to_string());
                    created
                }
            };

            new_text.push_str(&mask);
            last_idx = m.end();
        }

        new_text.push_str(&text[last_idx..]);
        new_text
    }

    /// Restaure l'ensemble des masques PII présents dans le texte complet
    /// vers leurs valeurs originales via la table de correspondance inverse.
    pub fn demask_text(&self, text: &str) -> String {
        let mut output = text.to_string();
        for (mask, original) in &self.reverse_map {
            output = output.replace(mask, original);
        }
        output
    }
}

/// Buffer glissant streaming SSE pour la reconstitution des tokens PII fractionnés (SPEC §3.1).
/// Garantit qu'un jeton découpé (ex: `["[EMAIL", "_1]"]`) est fidèlement reconstitué
/// et démasqué sans altérer la syntaxe normale de crochets (liens, citations `[[source: note.md]]`).
pub struct PiiSlidingBuffer {
    buffer: String,
    reverse_map: HashMap<String, String>,
}

impl PiiSlidingBuffer {
    pub fn new(reverse_map: HashMap<String, String>) -> Self {
        Self {
            buffer: String::new(),
            reverse_map,
        }
    }

    pub fn process_chunk(&mut self, chunk: &str) -> String {
        let mut result = String::new();
        let mut input = chunk;

        while !input.is_empty() {
            if !self.buffer.is_empty() {
                // Déjà en cours de mise en mémoire tampon d'un crochet ouvrant '['
                if let Some(close_idx) = input.find(']') {
                    self.buffer.push_str(&input[..=close_idx]);
                    input = &input[close_idx + 1..];

                    // Vérifier si le token correspond à un masque PII
                    if let Some(orig) = self.reverse_map.get(&self.buffer) {
                        result.push_str(orig);
                    } else {
                        result.push_str(&self.buffer);
                    }
                    self.buffer.clear();
                } else {
                    self.buffer.push_str(input);
                    if self.buffer.len() > 32 {
                        // Dépassement de la longueur maximale d'un jeton PII sans crochet fermant
                        result.push_str(&self.buffer);
                        self.buffer.clear();
                    }
                    break;
                }
            } else {
                // Pas de buffer actif : recherche du prochain crochet ouvrant '['
                if let Some(open_idx) = input.find('[') {
                    result.push_str(&input[..open_idx]);
                    let remainder = &input[open_idx..];

                    if let Some(close_idx) = remainder.find(']') {
                        let token = &remainder[..=close_idx];
                        if let Some(orig) = self.reverse_map.get(token) {
                            result.push_str(orig);
                        } else {
                            result.push_str(token);
                        }
                        input = &remainder[close_idx + 1..];
                    } else {
                        self.buffer.push_str(remainder);
                        if self.buffer.len() > 32 {
                            result.push_str(&self.buffer);
                            self.buffer.clear();
                        }
                        break;
                    }
                } else {
                    result.push_str(input);
                    break;
                }
            }
        }

        result
    }

    /// Vide le tampon à la clôture du flux streaming pour restituer tout reliquat sans perte.
    pub fn flush(&mut self) -> String {
        if self.buffer.is_empty() {
            String::new()
        } else {
            let pending = std::mem::take(&mut self.buffer);
            match self.reverse_map.get(&pending) {
                Some(orig) => orig.clone(),
                None => pending,
            }
        }
    }
}
