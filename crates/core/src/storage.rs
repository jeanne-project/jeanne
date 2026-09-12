use anyhow::Result;

pub struct DatabaseEngine;

impl DatabaseEngine {
    pub fn init() -> Result<Self> {
        tracing::info!("Initialisation du moteur de persistance SQLite et extensions vectorielles...");
        Ok(Self)
    }
}
