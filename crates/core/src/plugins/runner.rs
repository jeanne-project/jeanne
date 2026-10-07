//! Exécuteur asynchrone sécurisé de sous-processus JSON-RPC 2.0 avec Watchdog Tokio.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tracing::{debug, error, warn};

use super::models::{JsonRpcRequest, JsonRpcResponse, PluginError};

/// Taille maximale d'une ligne NDJSON émise par un plugin (8 Mo).
const MAX_LINE_BYTES: u64 = 8 * 1024 * 1024;

/// Exécute une commande JSON-RPC synchrone (request-response) sur un plugin avec Watchdog.
pub async fn execute_json_rpc(
    executable_path: &Path,
    working_dir: &Path,
    request: &JsonRpcRequest,
    timeout_duration: Duration,
) -> Result<JsonRpcResponse, PluginError> {
    debug!(
        "Lancement du sous-processus plugin : {} (méthode: {})",
        executable_path.display(),
        request.method
    );

    let mut cmd = Command::new(executable_path);
    cmd.current_dir(working_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Garantit l'éradication du processus sur tous les chemins de sortie (erreurs `?`).
        .kill_on_drop(true);

    let mut child = cmd.spawn().map_err(PluginError::Io)?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| PluginError::Io(std::io::Error::other("Impossible d'ouvrir stdin")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| PluginError::Io(std::io::Error::other("Impossible d'ouvrir stdout")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| PluginError::Io(std::io::Error::other("Impossible d'ouvrir stderr")))?;

    // Tâche d'arrière-plan pour journaliser stderr
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            debug!("[Plugin stderr] {}", line);
        }
    });

    // Sérialisation et transmission de la requête JSON-RPC sur stdin
    let payload = serde_json::to_string(request)?;
    stdin.write_all(payload.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await?;
    drop(stdin); // Fermeture du tube stdin pour signaler EOF si le plugin est en on_demand

    // Surveillance Watchdog sur stdout
    let mut stdout_reader = BufReader::new(stdout);

    let wait_output = async {
        let mut last_response: Option<JsonRpcResponse> = None;

        loop {
            // Lecture bornée : une ligne stdout ne peut dépasser MAX_LINE_BYTES (zéro buffer bloat).
            let mut buf = Vec::new();
            let n = (&mut stdout_reader)
                .take(MAX_LINE_BYTES + 1)
                .read_until(b'\n', &mut buf)
                .await?;
            if n == 0 {
                break;
            }
            if buf.len() as u64 > MAX_LINE_BYTES {
                return Err(PluginError::ProcessFailed(
                    None,
                    format!("ligne stdout supérieure à {MAX_LINE_BYTES} octets"),
                ));
            }
            let line = String::from_utf8_lossy(&buf);
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Tenter de désérialiser la ligne JSON-RPC
            if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(trimmed) {
                if resp.id == request.id {
                    last_response = Some(resp);
                    break;
                }
            }
        }

        let status = child.wait().await?;
        Ok::<_, PluginError>((last_response, status))
    };

    match tokio::time::timeout(timeout_duration, wait_output).await {
        Ok(Ok((Some(response), status))) => {
            if !status.success() && response.error.is_none() {
                warn!(
                    "Sous-processus terminé avec code de sortie non-nul : {:?}",
                    status.code()
                );
            }
            if let Some(err) = response.error {
                Err(PluginError::JsonRpc {
                    code: err.code,
                    message: err.message,
                })
            } else {
                Ok(response)
            }
        }
        Ok(Ok((None, status))) => {
            let code = status.code();
            Err(PluginError::ProcessFailed(
                code,
                "Le sous-processus s'est terminé sans renvoyer de réponse JSON-RPC valide"
                    .to_string(),
            ))
        }
        Ok(Err(e)) => Err(e),
        Err(_) => {
            error!(
                "Watchdog expiré pour {} après {}s. Éradication immédiate du sous-processus.",
                executable_path.display(),
                timeout_duration.as_secs()
            );
            // Élimination obligatoire du zombie
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(PluginError::Timeout(
                timeout_duration.as_secs(),
                executable_path.display().to_string(),
            ))
        }
    }
}
