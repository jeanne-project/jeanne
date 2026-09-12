use gray_matter::{engine::YAML, Matter};
use crate::error::{JeanneError, Result};
use crate::models::NoteFrontmatter;

/// Parse le contenu brut d'une note Markdown et sépare le frontmatter YAML du corps Markdown.
pub fn parse_markdown(content: &str) -> Result<(NoteFrontmatter, String)> {
    let matter = Matter::<YAML>::new();

    if let Some(parsed) = matter.parse_with_struct::<NoteFrontmatter>(content) {
        let body = parsed.content.trim_start_matches(['\r', '\n']).to_string();
        Ok((parsed.data, body))
    } else {
        let parsed = matter.parse(content);
        if parsed.data.is_none() {
            let body = parsed.content.trim_start_matches(['\r', '\n']).to_string();
            Ok((NoteFrontmatter::default(), body))
        } else {
            Err(JeanneError::Frontmatter(
                "Échec de désérialisation du frontmatter YAML dans la note".to_string(),
            ))
        }
    }
}
