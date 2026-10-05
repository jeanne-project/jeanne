#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, "..");

const capabilitiesPath = path.join(
  rootDir,
  "apps/desktop/src-tauri/capabilities/default.json"
);
const permissionsDir = path.join(
  rootDir,
  "apps/desktop/src-tauri/permissions"
);
const libRsPath = path.join(
  rootDir,
  "apps/desktop/src-tauri/src/lib.rs"
);

console.log("🔍 [Tauri Capabilities Linter] Contrôle de cohérence IPC & Permissions...");

// 1. Collecter tous les fichiers TOML de permissions
function collectTomlFiles(dir) {
  let files = [];
  if (!fs.existsSync(dir)) return files;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      files = files.concat(collectTomlFiles(fullPath));
    } else if (entry.isFile() && entry.name.endsWith(".toml")) {
      files.push(fullPath);
    }
  }
  return files;
}

const tomlFiles = collectTomlFiles(permissionsDir);
const definedPermissions = new Map(); // permId -> filepath

for (const filePath of tomlFiles) {
  const content = fs.readFileSync(filePath, "utf8");
  const matches = content.matchAll(/identifier\s*=\s*"([^"]+)"/g);
  for (const match of matches) {
    const permId = match[1];
    definedPermissions.set(permId, path.relative(rootDir, filePath));
  }
}

// 2. Lire les permissions dans capabilities/default.json
if (!fs.existsSync(capabilitiesPath)) {
  console.error(`❌ Erreur : Fichier introuvable : ${capabilitiesPath}`);
  process.exit(1);
}

const capContent = JSON.parse(fs.readFileSync(capabilitiesPath, "utf8"));
const declaredPermissions = capContent.permissions || [];

// 3. Extraire les commandes enregistrées dans lib.rs
if (!fs.existsSync(libRsPath)) {
  console.error(`❌ Erreur : Fichier introuvable : ${libRsPath}`);
  process.exit(1);
}

const libRsContent = fs.readFileSync(libRsPath, "utf8");
const handlerMatch = libRsContent.match(
  /invoke_handler\(tauri::generate_handler!\[([\s\S]*?)\]\)/
);

if (!handlerMatch) {
  console.error("❌ Erreur : Impossible de trouver 'invoke_handler(tauri::generate_handler![...])' dans lib.rs");
  process.exit(1);
}

const registeredCommands = handlerMatch[1]
  .split(",")
  .map((s) => s.trim())
  .filter((s) => s.length > 0 && !s.startsWith("//"));

let hasErrors = false;

// Vérification A : Chaque permission applicative dans capabilities/ doit exister dans les fichiers TOML
const appPermissions = declaredPermissions.filter((p) => !p.startsWith("core:"));

for (const perm of appPermissions) {
  if (!definedPermissions.has(perm)) {
    console.error(
      `❌ ERREUR PERMISSION : '${perm}' est déclarée dans capabilities/default.json mais aucun fichier .toml ne la définit sous permissions/`
    );
    hasErrors = true;
  }
}

// Vérification B : Chaque commande dans generate_handler! doit avoir sa permission allow-<kebab> dans capabilities et dans les TOML
for (const cmd of registeredCommands) {
  const expectedPerm = "allow-" + cmd.replace(/_/g, "-");

  if (!declaredPermissions.includes(expectedPerm)) {
    console.error(
      `❌ ERREUR CAPABILITY : La commande '${cmd}' est dans generate_handler! mais la permission '${expectedPerm}' manque dans capabilities/default.json`
    );
    hasErrors = true;
  }

  if (!definedPermissions.has(expectedPerm)) {
    console.error(
      `❌ ERREUR FICHIER PERMISSION : La commande '${cmd}' nécessite la permission '${expectedPerm}', mais celle-ci n'est définie dans aucun .toml sous permissions/`
    );
    hasErrors = true;
  }
}

if (hasErrors) {
  console.error("\n💥 Le contrôle des permissions Tauri a échoué. Corrigez les écarts ci-dessus avant de commiter.");
  process.exit(1);
}

console.log(
  `✅ Permissions Tauri vérifiées avec succès : ${registeredCommands.length} commandes IPC, ${appPermissions.length} permissions applicatives validées dans ${tomlFiles.length} fichiers TOML.`
);
