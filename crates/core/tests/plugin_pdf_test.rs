//! Jalon 7 — TEST-07-01..03 : parser PDF, watchdog sans zombie, immunité aux crashs.

use jeanne_core::plugins::{
    JsonRpcRequest, PluginError, PluginManager, PluginManifest, execute_json_rpc,
};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

fn ensure_plugin_executable(manifest: &PluginManifest) -> Option<PathBuf> {
    if let Ok(exe) = PluginManager::resolve_executable(manifest) {
        return Some(exe);
    }
    let target = manifest
        .root_dir
        .join(manifest.entrypoint.resolve_for_current_os());
    if let Some(parent) = target.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let out = Command::new("go")
        .args(["build", "-ldflags=-s -w", "-o"])
        .arg(&target)
        .arg(".")
        .current_dir(&manifest.root_dir)
        .output()
        .ok()?;
    (out.status.success() && target.exists()).then_some(target)
}

/// Construit un PDF minimal (Helvetica + /Widths) avec titre, paragraphe et tableau.
fn build_sample_pdf() -> Vec<u8> {
    let widths: Vec<String> = (32..=126)
        .map(|c| if c == 32 { "278" } else { "556" }.to_string())
        .collect();
    let ops = [
        (24, 72, 720, "Financial Report 2026"),
        (12, 72, 690, "Revenue increased by 14 percent."),
        (16, 72, 640, "Key Figures"),
        (12, 72, 610, "Region"),
        (12, 250, 610, "Revenue"),
        (12, 72, 595, "Europe"),
        (12, 250, 595, "120"),
    ];
    let mut stream = String::from("BT\n");
    for (size, x, y, s) in ops {
        stream.push_str(&format!("/F1 {size} Tf 1 0 0 1 {x} {y} Tm ({s}) Tj\n"));
    }
    stream.push_str("ET\n");
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
        format!(
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 32 /LastChar 126 /Widths [{}] >>",
            widths.join(" ")
        ),
        "<< /Title (Financial Report 2026) /Author (CFO Office) /CreationDate (D:20260315120000Z) >>".to_string(),
    ];
    let mut buf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(buf.len());
        buf.push_str(&format!("{} 0 obj\n{o}\nendobj\n", i + 1));
    }
    let xref = buf.len();
    buf.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objs.len() + 1
    ));
    for o in offsets {
        buf.push_str(&format!("{o:010} 00000 n \n"));
    }
    buf.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objs.len() + 1
    ));
    buf.into_bytes()
}

#[cfg(unix)]
fn write_script(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    path
}

#[tokio::test]
async fn test_07_01_pdf_parser_roundtrip() {
    let mut manager = PluginManager::new();
    manager.discover_plugins();
    let manifest = manager
        .find_by_capability("document_parser")
        .expect("plugin pdf-parser doit être découvert");
    let Some(exe) = ensure_plugin_executable(manifest) else {
        eprintln!("SKIP: compilateur go indisponible");
        return;
    };

    let dir = tempfile::tempdir().unwrap();
    let pdf_path = dir.path().join("report.pdf");
    std::fs::write(&pdf_path, build_sample_pdf()).unwrap();

    let req = JsonRpcRequest::new(
        "parse_document",
        serde_json::json!({
            "file_path": pdf_path.to_string_lossy(),
            "options": { "extract_tables": true, "extract_images": false }
        }),
        1,
    );
    let resp = execute_json_rpc(&exe, &manifest.root_dir, &req, Duration::from_secs(20))
        .await
        .expect("parse_document doit réussir");
    let res = resp.result.expect("result attendu");
    let md = res["content_markdown"].as_str().unwrap();
    assert!(md.contains("# Financial Report 2026"), "{md}");
    assert!(md.contains("## Key Figures"), "{md}");
    assert!(md.contains("| Region | Revenue |"), "{md}");
    assert!(md.contains("| Europe | 120 |"), "{md}");
    assert_eq!(res["title"], "Financial Report 2026");
    assert_eq!(res["metadata"]["author"], "CFO Office");
    assert_eq!(res["metadata"]["date"], "2026-03-15");
    assert_eq!(res["metadata"]["page_count"], 1);

    // Fichier inexistant -> -32001 ; fichier corrompu -> -32002
    let missing = JsonRpcRequest::new(
        "parse_document",
        serde_json::json!({ "file_path": dir.path().join("absent.pdf").to_string_lossy() }),
        2,
    );
    match execute_json_rpc(&exe, &manifest.root_dir, &missing, Duration::from_secs(20)).await {
        Err(PluginError::JsonRpc { code, .. }) => assert_eq!(code, -32001),
        other => panic!("attendu -32001, obtenu {other:?}"),
    }
    let bad = dir.path().join("bad.pdf");
    std::fs::write(&bad, b"not a pdf").unwrap();
    let corrupted = JsonRpcRequest::new(
        "parse_document",
        serde_json::json!({ "file_path": bad.to_string_lossy() }),
        3,
    );
    match execute_json_rpc(
        &exe,
        &manifest.root_dir,
        &corrupted,
        Duration::from_secs(20),
    )
    .await
    {
        Err(PluginError::JsonRpc { code, .. }) => assert_eq!(code, -32002),
        other => panic!("attendu -32002, obtenu {other:?}"),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn test_07_02_watchdog_leaves_no_zombie() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("pid");
    let script = write_script(
        dir.path(),
        "hang.sh",
        &format!(
            "#!/bin/sh\necho $$ > {}\nexec sleep 999\n",
            pid_file.display()
        ),
    );
    let req = JsonRpcRequest::new("parse_document", serde_json::json!({}), 1);
    let res = execute_json_rpc(&script, dir.path(), &req, Duration::from_secs(2)).await;
    assert!(matches!(res, Err(PluginError::Timeout(..))), "{res:?}");

    let pid = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .to_string();
    // Ni processus vivant ni zombie : /proc/<pid> doit avoir disparu (processus réapé).
    assert!(
        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "le processus {pid} subsiste après le watchdog"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_07_03_crash_immunity() {
    let dir = tempfile::tempdir().unwrap();
    let script = write_script(dir.path(), "crash.sh", "#!/bin/sh\nkill -SEGV $$\n");
    let req = JsonRpcRequest::new("parse_document", serde_json::json!({}), 1);
    let res = execute_json_rpc(&script, dir.path(), &req, Duration::from_secs(5)).await;
    match res {
        Err(PluginError::ProcessFailed(code, _)) => {
            // Terminé par signal : pas de code de sortie classique.
            assert!(code.is_none() || code != Some(0));
        }
        other => panic!("attendu ProcessFailed, obtenu {other:?}"),
    }
    // Le processus de test (cœur) reste pleinement opérationnel.
    let ok = write_script(
        dir.path(),
        "ok.sh",
        "#!/bin/sh\nread l\necho '{\"jsonrpc\":\"2.0\",\"result\":{\"pong\":true},\"id\":1}'\n",
    );
    let resp = execute_json_rpc(&ok, dir.path(), &req, Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(resp.result.unwrap()["pong"], true);
}
