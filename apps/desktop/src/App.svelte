<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import QuickAccess from './lib/components/QuickAccess.svelte';
  import type { VaultStats, HardwareInfo, LocalInferenceStats } from './lib/types/ipc';

  let windowLabel = $state('main');
  let coreVersion = $state('Chargement...');
  let vaultStats = $state<VaultStats | null>(null);
  let hardwareInfo = $state<HardwareInfo | null>(null);
  let inferenceStats = $state<LocalInferenceStats | null>(null);
  let isModelLoading = $state(false);

  // Modals
  let showHelpModal = $state(false);
  let showModelSetupModal = $state(false);
  let modelSetupPath = $state('');
  let modelSetupError = $state('');
  let pathCopied = $state(false);

  const isQuickAccess = $derived(windowLabel === 'quick-access');

  async function refreshHardware() {
    try {
      hardwareInfo = await invoke<HardwareInfo>('get_hardware_profile');
      inferenceStats = await invoke<LocalInferenceStats>('get_local_inference_stats');
    } catch {
      // Ignoré si mode web pur
    }
  }

  async function toggleLocalModel() {
    if (!hardwareInfo) return;
    isModelLoading = true;
    try {
      if (hardwareInfo.recommended_model_loaded) {
        await invoke('unload_local_model');
      } else {
        await invoke('load_local_model');
      }
      await refreshHardware();
    } catch (e: unknown) {
      const errMsg = String(e);
      // Si le modèle est introuvable → afficher le guide d'installation
      if (errMsg.includes('does not exist') || errMsg.includes('not found') || errMsg.includes('No such file')) {
        try {
          modelSetupPath = await invoke<string>('get_default_model_path');
        } catch {
          modelSetupPath = '(chemin non disponible)';
        }
        modelSetupError = errMsg;
        showModelSetupModal = true;
      } else {
        console.error('Erreur chargement modèle local', e);
      }
    } finally {
      isModelLoading = false;
    }
  }

  async function copyPath() {
    try {
      await navigator.clipboard.writeText(modelSetupPath);
      pathCopied = true;
      setTimeout(() => { pathCopied = false; }, 2000);
    } catch {
      // Fallback si clipboard non disponible
    }
  }

  $effect(() => {
    let label = 'main';
    try {
      const win = getCurrentWebviewWindow();
      label = win.label;
    } catch {
      label = 'main';
    }
    windowLabel = label;

    if (label === 'quick-access') {
      document.documentElement.classList.add('quick-access-window');
      document.body.classList.add('quick-access-window');
      document.body.classList.remove('main-window');
    } else {
      document.documentElement.classList.remove('quick-access-window');
      document.body.classList.remove('quick-access-window');
      document.body.classList.add('main-window');

      invoke<string>('get_core_version')
        .then((ver) => {
          coreVersion = ver;
        })
        .catch(() => {
          coreVersion = 'Erreur connexion core';
        });

      invoke<VaultStats>('get_vault_stats')
        .then((stats) => {
          vaultStats = stats;
        })
        .catch(() => {
          vaultStats = null;
        });

      refreshHardware();
    }
  });

  async function handleExitApp() {
    try {
      await invoke('exit_app');
    } catch {
      window.close();
    }
  }
</script>

<svelte:window
  onkeydown={(e) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'q') {
      e.preventDefault();
      handleExitApp();
    }
    if (e.key === 'Escape') {
      showHelpModal = false;
      showModelSetupModal = false;
    }
  }}
/>

{#if isQuickAccess}
  <QuickAccess mode="overlay" />
{:else}
  <main class="main-dashboard">
    <header class="header">
      <div class="brand-row">
        <div class="brand-left">
          <h1 class="logo-title">Jeanne</h1>
          <span class="version-badge">v{coreVersion}</span>
        </div>
        <div class="header-actions">
          <div class="header-shortcut">
            <span class="shortcut-pill-label">Accès Rapide :</span>
            <div class="shortcut-pill">
              <kbd>Alt</kbd> + <kbd>Espace</kbd>
            </div>
          </div>
          <button
            type="button"
            class="help-button"
            onclick={() => { showHelpModal = true; }}
            title="Guide d'utilisation de Jeanne"
            aria-label="Ouvrir l'aide"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
              <circle cx="12" cy="12" r="10"></circle>
              <path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3"></path>
              <line x1="12" y1="17" x2="12.01" y2="17"></line>
            </svg>
            <span>Aide</span>
          </button>
          <button
            type="button"
            class="quit-button"
            onclick={handleExitApp}
            title="Fermer et quitter Jeanne (Ctrl + Q)"
            aria-label="Fermer et quitter l'application"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
              <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"></path>
              <polyline points="16 17 21 12 16 7"></polyline>
              <line x1="21" y1="12" x2="9" y2="12"></line>
            </svg>
            <span>Quitter</span>
          </button>
        </div>
      </div>
      <p class="tagline">Assistant de connaissances souverain et co-pilote local ("File-over-App").</p>
    </header>

    <!-- Zone Principale de Recherche et Prise de Note Rapide -->
    <section class="search-section" aria-label="Recherche et capture de notes">
      <QuickAccess mode="embedded" />
    </section>

    <!-- Panneaux d'Informations et Métriques -->
    <div class="dashboard-grid">
      <!-- Statistiques du Coffre -->
      <section class="stats-section" aria-label="Statistiques du coffre">
        <h2 class="section-title">Statistiques du Coffre</h2>
        <div class="stats-cards">
          <div class="stat-card">
            <span class="stat-label">Fichiers Indexés</span>
            <span class="stat-value">{vaultStats?.total_files ?? 0}</span>
          </div>
          <div class="stat-card">
            <span class="stat-label">Fragments (Chunks)</span>
            <span class="stat-value">{vaultStats?.total_chunks ?? 0}</span>
          </div>
          <div class="stat-card">
            <span class="stat-label">Dernier Scan</span>
            <span class="stat-value">
              {vaultStats?.last_scan_timestamp ? new Date(vaultStats.last_scan_timestamp * 1000).toLocaleTimeString() : 'Jamais'}
            </span>
          </div>
        </div>
      </section>

      <!-- Moteur d'Inférence Local (GGUF / Vulkan) -->
      <section class="model-section" aria-label="Moteur d'inférence local">
        <div class="model-header">
          <h2 class="section-title">Inférence Locale (Vulkan / GGUF)</h2>
          <span class="status-badge {hardwareInfo?.recommended_model_loaded ? 'status-active' : 'status-idle'}">
            {hardwareInfo?.recommended_model_loaded ? 'Modèle Chargé (3B)' : 'Modèle Déchargé'}
          </span>
        </div>
        <p class="model-description">
          Exécution souveraine 100% hors-ligne. Modèle cible : <code>Qwen2.5-3B-Instruct-Q4_K_M</code>.
          Fonctionne sans connexion internet ni envoi de données.
        </p>
        <div class="stats-cards">
          <div class="stat-card">
            <span class="stat-label">RAM Système (Dispo / Total)</span>
            <span class="stat-value">{hardwareInfo ? `${Math.round(hardwareInfo.available_ram_mb / 1024)}G / ${Math.round(hardwareInfo.total_system_ram_mb / 1024)}G` : '...'}</span>
          </div>
          <div class="stat-card">
            <span class="stat-label">Accélération Vulkan</span>
            <span class="stat-value">{hardwareInfo?.vulkan_supported ? (hardwareInfo.vulkan_device_name ?? 'Actif') : 'Non détecté'}</span>
          </div>
          <div class="stat-card">
            <span class="stat-label">Empreinte RAM Modèle</span>
            <span class="stat-value">{inferenceStats?.memory_allocated_mb ?? 0} Mo</span>
          </div>
        </div>
        <div class="model-actions">
          <button
            type="button"
            class="model-toggle-btn {hardwareInfo?.recommended_model_loaded ? 'btn-unload' : 'btn-load'}"
            onclick={toggleLocalModel}
            disabled={isModelLoading}
          >
            {#if isModelLoading}
              <span class="spinner"></span>
              <span>Opération en cours...</span>
            {:else if hardwareInfo?.recommended_model_loaded}
              <span>Décharger le Modèle (&lt; 200 Mo RAM)</span>
            {:else}
              <span>Charger le Modèle Local (Qwen 3B)</span>
            {/if}
          </button>
          <button
            type="button"
            class="model-info-btn"
            onclick={() => { showModelSetupModal = true; invoke<string>('get_default_model_path').then(p => { modelSetupPath = p; modelSetupError = ''; }).catch(() => {}); }}
            title="Voir les instructions d'installation du modèle"
          >
            ℹ️ Guide d'installation
          </button>
        </div>
      </section>

      <!-- Palette d'Accès Rapide -->
      <section class="overlay-info" aria-label="Informations sur la palette d'accès rapide">
        <h2 class="section-title">Palette d'Accès Rapide</h2>
        <p>Invoquez la palette flottante au premier plan à tout instant depuis n'importe quelle application :</p>
        <div class="shortcut-box">
          <kbd>Alt</kbd> + <kbd>Espace</kbd>
        </div>
        <div class="hint">
          <span class="hint-icon">ℹ️</span>
          <span>Repli automatique sur <kbd>Alt</kbd> + <kbd>Maj</kbd> + <kbd>Espace</kbd> si déjà réservé par le système.</span>
        </div>
      </section>
    </div>
  </main>
{/if}

<!-- ─── Modal : Guide d'installation du modèle ─── -->
{#if showModelSetupModal}
  <div class="modal-backdrop" onclick={() => { showModelSetupModal = false; }} role="presentation">
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="modal" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-labelledby="modal-setup-title" tabindex="-1">
      <div class="modal-header">
        <h2 id="modal-setup-title" class="modal-title">📥 Installation du Modèle Local</h2>
        <button class="modal-close" onclick={() => { showModelSetupModal = false; }} aria-label="Fermer">✕</button>
      </div>
      <div class="modal-body">
        {#if modelSetupError}
          <div class="alert-box">
            <strong>⚠️ Modèle introuvable</strong><br/>
            Le fichier GGUF n'a pas été trouvé à l'emplacement attendu.
          </div>
        {/if}

        <h3 class="setup-step-title">Étape 1 — Télécharger le modèle</h3>
        <p class="setup-text">Téléchargez le fichier <code>Qwen2.5-3B-Instruct-Q4_K_M.gguf</code> (~2.1 Go) depuis Hugging Face :</p>
        <a
          class="download-link"
          href="https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf"
          target="_blank"
          rel="noopener noreferrer"
        >
          🤗 Hugging Face — Qwen2.5-3B-Instruct-Q4_K_M.gguf
        </a>
        <p class="setup-hint">Connectez-vous sur Hugging Face si demandé. Téléchargement direct (~2.1 Go).</p>

        <h3 class="setup-step-title">Étape 2 — Placer le fichier</h3>
        <p class="setup-text">Déposez le fichier <strong>sans renommer</strong> dans le répertoire suivant :</p>
        <div class="path-box">
          <code class="path-text">{modelSetupPath || 'Chargement du chemin...'}</code>
          <button class="copy-btn" onclick={copyPath} title="Copier le chemin">
            {pathCopied ? '✓ Copié !' : '📋 Copier'}
          </button>
        </div>
        <p class="setup-hint">Créez le dossier <code>models/</code> s'il n'existe pas encore.</p>

        <h3 class="setup-step-title">Étape 3 — Charger dans Jeanne</h3>
        <p class="setup-text">Cliquez sur <strong>« Charger le Modèle Local (Qwen 3B) »</strong> dans le tableau de bord. Le chargement prend quelques secondes (~2.1 Go en mémoire).</p>

        <div class="setup-requirements">
          <strong>⚙️ Configuration recommandée :</strong>
          <ul>
            <li>RAM : 8 Go disponibles minimum (16 Go total recommandé)</li>
            <li>GPU : Vulkan compatible (Intel Iris Xe, AMD Radeon, NVIDIA GeForce)</li>
            <li>Espace disque : ~2.2 Go pour le fichier GGUF</li>
          </ul>
        </div>
      </div>
      <div class="modal-footer">
        <button class="btn-primary" onclick={() => { showModelSetupModal = false; }}>Compris, je télécharge</button>
      </div>
    </div>
  </div>
{/if}

<!-- ─── Modal : Aide / Documentation utilisateur ─── -->
{#if showHelpModal}
  <div class="modal-backdrop" onclick={() => { showHelpModal = false; }} role="presentation">
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="modal modal-wide" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-labelledby="modal-help-title" tabindex="-1">
      <div class="modal-header">
        <h2 id="modal-help-title" class="modal-title">📖 Guide d'utilisation de Jeanne</h2>
        <button class="modal-close" onclick={() => { showHelpModal = false; }} aria-label="Fermer">✕</button>
      </div>
      <div class="modal-body help-body">

        <section class="help-section">
          <h3 class="help-section-title">🔍 Recherche dans votre coffre de notes</h3>
          <p>Jeanne indexe automatiquement tous vos fichiers Markdown (<code>.md</code>) dans votre coffre. La recherche combine :</p>
          <ul>
            <li><strong>Recherche lexicale BM25</strong> — mots-clés exacts, rapide (&lt; 15 ms)</li>
            <li><strong>Recherche sémantique vectorielle</strong> — sens et contexte (RAG hybride)</li>
            <li><strong>Priorité temporelle</strong> — les notes récentes remontent naturellement</li>
          </ul>
          <div class="help-tip">Tapez simplement votre requête dans la barre de recherche. Pas besoin de commande spéciale.</div>
        </section>

        <section class="help-section">
          <h3 class="help-section-title">⚡ Palette d'Accès Rapide</h3>
          <p>Invoquez Jeanne depuis <em>n'importe quelle application</em> sans quitter votre contexte de travail :</p>
          <div class="shortcut-box">
            <kbd>Alt</kbd> + <kbd>Espace</kbd>
          </div>
          <ul>
            <li>La palette s'ouvre en <strong>moins de 50 ms</strong>, toujours au premier plan</li>
            <li>Appuyez sur <kbd>Échap</kbd> pour la fermer</li>
            <li>Raccourci de repli : <kbd>Alt</kbd> + <kbd>Maj</kbd> + <kbd>Espace</kbd></li>
          </ul>
        </section>

        <section class="help-section">
          <h3 class="help-section-title">🚀 Commandes Slash & Suite de Productivité</h3>
          <p>Tapez <code>/</code> dans la palette pour ouvrir le menu d'actions rapides :</p>
          <div class="shortcuts-table">
            <div class="shortcut-row"><kbd>/todo [texte]</kbd><span>Ajouter une tâche à <code>Inbox.md</code></span></div>
            <div class="shortcut-row"><kbd>/tasks</kbd><span>Lister et cocher les tâches en cours directement dans la palette</span></div>
            <div class="shortcut-row"><kbd>/note [texte]</kbd><span>Consigner une note horodatée dans le Journal du jour</span></div>
            <div class="shortcut-row"><kbd>/log [texte]</kbd><span>Micro-journaling horodaté (time-tracking de la journée)</span></div>
            <div class="shortcut-row"><kbd>/meeting [titre]</kbd><span>Générer un compte-rendu de réunion structuré</span></div>
            <div class="shortcut-row"><kbd>/bookmark [url]</kbd><span>Enregistrer un signet web dans <code>Bookmarks.md</code></span></div>
            <div class="shortcut-row"><kbd>/snip</kbd><span>Insérer un modèle de texte réutilisable (mail pro, trame...)</span></div>
            <div class="shortcut-row"><kbd>/timer [durée]</kbd><span>Lancer un minuteur (ex: <code>/timer 25m Pause café</code>)</span></div>
            <div class="shortcut-row"><kbd>/clip</kbd><span>Consulter et réutiliser l'historique du presse-papier</span></div>
            <div class="shortcut-row"><kbd>/scratch</kbd><span>Ouvrir un bloc-notes brouillon éphémère</span></div>
          </div>
        </section>

        <section class="help-section">
          <h3 class="help-section-title">🧮 Calculatrice Arithmétique Inline</h3>
          <p>Tapez directement une formule dans la barre de recherche (ex: <code>12 * 4.5</code>, <code>(100 + 20) / 4</code>, <code>2^8</code>) :</p>
          <div class="code-example"><code>145 * 1.2  →  = 174</code></div>
          <p>Appuyez sur <kbd>Entrée</kbd> pour copier instantanément le résultat dans votre presse-papier.</p>
        </section>

        <section class="help-section">
          <h3 class="help-section-title">🤖 Actions IA Presse-Papier & Coffre</h3>
          <p>Utilisez l'IA locale (Qwen 3B) ou distante pour agir directement sur ce que vous venez de copier :</p>
          <div class="shortcuts-table">
            <div class="shortcut-row"><kbd>/corrige</kbd><span>Corrige l'orthographe et la syntaxe du texte copié</span></div>
            <div class="shortcut-row"><kbd>/rephrase [ton]</kbd><span>Reformule le texte copié (ex: <code>/rephrase pro</code>)</span></div>
            <div class="shortcut-row"><kbd>/tldr</kbd><span>Résume le texte copié en 3 puces synthétiques</span></div>
            <div class="shortcut-row"><kbd>/trad [langue]</kbd><span>Traduit le texte copié sans quitter votre écran</span></div>
            <div class="shortcut-row"><kbd>/ask [question]</kbd><span>Interroge votre coffre et synthétise la réponse (RAG)</span></div>
          </div>
        </section>

        <section class="help-section">
          <h3 class="help-section-title">🤖 Inférence IA Distante (OpenAI compatible)</h3>
          <p>Connectez Jeanne à tout endpoint compatible OpenAI (<code>/v1/chat/completions</code>) :</p>
          <ul>
            <li>OpenAI GPT-4, Mistral, Groq, LM Studio, Ollama…</li>
            <li>Réponses en <strong>streaming temps réel</strong></li>
            <li>Protection automatique des données sensibles (e-mails, téléphones) avant envoi</li>
            <li>Sources citées : <code>[source: nom_note.md]</code></li>
          </ul>
        </section>

        <section class="help-section">
          <h3 class="help-section-title">🖥️ Inférence IA Locale (100% hors-ligne)</h3>
          <p>Faites tourner un modèle IA directement sur votre machine, <strong>sans aucune connexion internet</strong> :</p>
          <ul>
            <li>Modèle : <code>Qwen2.5-3B-Instruct-Q4_K_M.gguf</code> (~2.1 Go)</li>
            <li>Accélération matérielle Vulkan (Intel/AMD/NVIDIA)</li>
            <li>Contexte plafonné à 4096 tokens pour maîtriser la RAM</li>
            <li>Compression automatique des prompts longs</li>
            <li>Vos données ne quittent jamais votre machine</li>
          </ul>
          <div class="help-tip">
            Cliquez sur <strong>« Guide d'installation »</strong> dans la section Inférence Locale pour télécharger le modèle.
          </div>
        </section>

        <section class="help-section">
          <h3 class="help-section-title">📁 Philosophie "File-over-App"</h3>
          <p>Jeanne ne stocke rien dans une base propriétaire. <strong>Vos notes Markdown restent vos fichiers</strong> :</p>
          <ul>
            <li>Lisibles dans n'importe quel éditeur (Obsidian, VS Code, Notepad…)</li>
            <li>L'index SQLite est un <em>cache reconstituable</em> à tout moment</li>
            <li>Supprimez <code>.jeanne/database.db</code> : Jeanne reconstruit tout au redémarrage</li>
          </ul>
        </section>

        <section class="help-section">
          <h3 class="help-section-title">⌨️ Raccourcis Clavier</h3>
          <div class="shortcuts-table">
            <div class="shortcut-row"><kbd>Alt</kbd> + <kbd>Espace</kbd><span>Ouvrir / fermer la palette flottante</span></div>
            <div class="shortcut-row"><kbd>Ctrl</kbd> + <kbd>Q</kbd><span>Quitter Jeanne</span></div>
            <div class="shortcut-row"><kbd>Échap</kbd><span>Fermer la palette / cette fenêtre</span></div>
          </div>
        </section>

      </div>
      <div class="modal-footer">
        <button class="btn-primary" onclick={() => { showHelpModal = false; }}>Fermer</button>
      </div>
    </div>
  </div>
{/if}

<style>
  :global(body) {
    background-color: #0d1117;
    color: #e6edf3;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    margin: 0;
    padding: 0;
  }

  .main-dashboard {
    max-width: 860px;
    margin: 0 auto;
    padding: 2.5rem 1.5rem 4rem 1.5rem;
    display: flex;
    flex-direction: column;
    gap: 2rem;
    box-sizing: border-box;
  }

  .header {
    margin-bottom: 0;
  }

  .brand-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 1rem;
    flex-wrap: wrap;
  }

  .brand-left {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .logo-title {
    font-size: 2.2rem;
    font-weight: 800;
    letter-spacing: -0.02em;
    margin: 0;
    background: linear-gradient(135deg, #a5b4fc, #6366f1);
    background-clip: text;
    -webkit-background-clip: text;
    -webkit-text-fill-color: transparent;
  }

  .version-badge {
    background: rgba(99, 102, 241, 0.18);
    color: #c7d2fe;
    font-size: 0.8rem;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: 6px;
    border: 1px solid rgba(99, 102, 241, 0.35);
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    flex-wrap: wrap;
  }

  .quit-button,
  .help-button {
    display: inline-flex;
    align-items: center;
    gap: 0.45rem;
    padding: 0.45rem 0.85rem;
    border-radius: 8px;
    font-size: 0.82rem;
    font-weight: 600;
    cursor: pointer;
    font-family: inherit;
    transition: background 0.15s ease, border-color 0.15s ease, color 0.15s ease;
  }

  .help-button {
    background: rgba(99, 102, 241, 0.12);
    border: 1px solid rgba(99, 102, 241, 0.3);
    color: #c7d2fe;
  }

  .help-button:hover {
    background: rgba(99, 102, 241, 0.22);
    border-color: rgba(99, 102, 241, 0.5);
    color: #e0e7ff;
  }

  .quit-button {
    background: rgba(239, 68, 68, 0.12);
    border: 1px solid rgba(239, 68, 68, 0.3);
    color: #fca5a5;
  }

  .quit-button:hover {
    background: rgba(239, 68, 68, 0.22);
    border-color: rgba(239, 68, 68, 0.5);
    color: #fecaca;
  }

  .header-shortcut {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    background: #161b22;
    border: 1px solid #30363d;
    padding: 0.45rem 0.9rem;
    border-radius: 8px;
  }

  .shortcut-pill-label {
    font-size: 0.82rem;
    color: #8b949e;
    font-weight: 500;
  }

  .shortcut-pill {
    display: flex;
    align-items: center;
    gap: 0.25rem;
  }

  .shortcut-pill kbd {
    background: #21262d;
    color: #f0f6fc;
    border: 1px solid #484f58;
    border-radius: 4px;
    padding: 2px 7px;
    font-size: 0.8rem;
    font-weight: 600;
  }

  .tagline {
    color: #8b949e;
    margin-top: 0.4rem;
    margin-bottom: 0;
    font-size: 1.05rem;
  }

  .search-section {
    width: 100%;
    margin: 0 auto;
  }

  .dashboard-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 1.5rem;
  }

  @media (max-width: 768px) {
    .dashboard-grid {
      grid-template-columns: 1fr;
    }
  }

  .section-title {
    font-size: 1.15rem;
    font-weight: 700;
    color: #f0f6fc;
    margin: 0 0 1rem 0;
  }

  .stats-section {
    background: #161b22;
    border: 1px solid #30363d;
    border-radius: 12px;
    padding: 1.5rem;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
  }

  .stats-cards {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 0.75rem;
  }

  .stat-card {
    background: #0d1117;
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 8px;
    padding: 1rem 0.75rem;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .stat-label {
    font-size: 0.72rem;
    color: #8b949e;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .stat-value {
    font-size: 1.1rem;
    font-weight: 700;
    color: #f0f6fc;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .overlay-info {
    background: #161b22;
    border: 1px solid #30363d;
    border-radius: 12px;
    padding: 1.5rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  .overlay-info p {
    margin: 0;
    color: #c9d1d9;
    font-size: 0.92rem;
    line-height: 1.45;
  }

  .shortcut-box {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.5rem 1rem;
    background: rgba(99, 102, 241, 0.15);
    border: 1px solid rgba(99, 102, 241, 0.4);
    border-radius: 8px;
    font-size: 1rem;
    color: #e0e7ff;
    width: fit-content;
    margin: 0.2rem 0;
  }

  .shortcut-box kbd {
    background: #1e2238;
    color: #c7d2fe;
    border: 1px solid rgba(99, 102, 241, 0.45);
    border-radius: 4px;
    padding: 2px 7px;
    font-weight: 600;
  }

  .hint {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    flex-wrap: wrap;
    font-size: 0.88rem;
    color: #e2e8f0;
    margin-top: 0.3rem;
    padding: 0.65rem 0.85rem;
    background: #0d1117;
    border-radius: 6px;
    border: 1px solid #30363d;
    line-height: 1.45;
  }

  .hint-icon {
    font-size: 0.95rem;
  }

  .hint kbd {
    background: #21262d;
    color: #f0f6fc;
    border: 1px solid #484f58;
    border-radius: 4px;
    padding: 2px 6px;
    font-family: inherit;
    font-size: 0.85em;
    font-weight: 600;
  }

  .model-section {
    background: #161b22;
    border: 1px solid #30363d;
    border-radius: 12px;
    padding: 1.5rem;
    display: flex;
    flex-direction: column;
    gap: 1rem;
    grid-column: 1 / -1;
  }

  .model-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 1rem;
  }

  .model-description {
    margin: 0;
    color: #8b949e;
    font-size: 0.88rem;
    line-height: 1.5;
  }

  .model-description code {
    background: rgba(99, 102, 241, 0.15);
    color: #c7d2fe;
    padding: 2px 6px;
    border-radius: 4px;
    font-size: 0.85em;
  }

  .status-badge {
    font-size: 0.75rem;
    font-weight: 600;
    padding: 3px 10px;
    border-radius: 20px;
    letter-spacing: 0.02em;
    white-space: nowrap;
  }

  .status-active {
    background: rgba(34, 197, 94, 0.18);
    color: #86efac;
    border: 1px solid rgba(34, 197, 94, 0.35);
  }

  .status-idle {
    background: rgba(148, 163, 184, 0.12);
    color: #94a3b8;
    border: 1px solid rgba(148, 163, 184, 0.25);
  }

  .model-actions {
    display: flex;
    gap: 0.75rem;
    margin-top: 0.25rem;
    flex-wrap: wrap;
    align-items: center;
  }

  .model-toggle-btn {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.6rem 1.25rem;
    border-radius: 8px;
    font-size: 0.85rem;
    font-weight: 600;
    cursor: pointer;
    font-family: inherit;
    transition: all 0.2s ease;
    border: none;
  }

  .model-info-btn {
    padding: 0.55rem 1rem;
    border-radius: 8px;
    font-size: 0.82rem;
    font-weight: 500;
    cursor: pointer;
    font-family: inherit;
    background: rgba(148, 163, 184, 0.08);
    border: 1px solid rgba(148, 163, 184, 0.2);
    color: #94a3b8;
    transition: all 0.15s ease;
  }

  .model-info-btn:hover {
    background: rgba(148, 163, 184, 0.15);
    color: #cbd5e1;
  }

  .btn-load {
    background: #4f46e5;
    color: #ffffff;
  }

  .btn-load:hover:not(:disabled) {
    background: #4338ca;
  }

  .btn-unload {
    background: rgba(239, 68, 68, 0.15);
    color: #fca5a5;
    border: 1px solid rgba(239, 68, 68, 0.35) !important;
  }

  .btn-unload:hover:not(:disabled) {
    background: rgba(239, 68, 68, 0.25);
  }

  .model-toggle-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  /* Spinner */
  .spinner {
    width: 14px;
    height: 14px;
    border: 2px solid rgba(255, 255, 255, 0.3);
    border-top-color: #fff;
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
    display: inline-block;
    flex-shrink: 0;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  /* ─── Modals ─── */
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.72);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
    padding: 1rem;
    backdrop-filter: blur(4px);
  }

  .modal {
    background: #161b22;
    border: 1px solid #30363d;
    border-radius: 14px;
    max-width: 560px;
    width: 100%;
    max-height: 88vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.6);
  }

  .modal-wide {
    max-width: 720px;
  }

  .modal-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 1.25rem 1.5rem;
    border-bottom: 1px solid #30363d;
    flex-shrink: 0;
  }

  .modal-title {
    font-size: 1.1rem;
    font-weight: 700;
    color: #f0f6fc;
    margin: 0;
  }

  .modal-close {
    background: none;
    border: none;
    color: #8b949e;
    font-size: 1.1rem;
    cursor: pointer;
    padding: 0.25rem 0.5rem;
    border-radius: 6px;
    transition: color 0.15s, background 0.15s;
  }

  .modal-close:hover {
    color: #f0f6fc;
    background: rgba(255, 255, 255, 0.08);
  }

  .modal-body {
    padding: 1.5rem;
    overflow-y: auto;
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 1.2rem;
  }

  .modal-footer {
    padding: 1rem 1.5rem;
    border-top: 1px solid #30363d;
    display: flex;
    justify-content: flex-end;
    flex-shrink: 0;
  }

  .btn-primary {
    background: #4f46e5;
    color: #fff;
    border: none;
    padding: 0.6rem 1.4rem;
    border-radius: 8px;
    font-size: 0.88rem;
    font-weight: 600;
    cursor: pointer;
    font-family: inherit;
    transition: background 0.15s;
  }

  .btn-primary:hover {
    background: #4338ca;
  }

  /* Setup modal */
  .alert-box {
    background: rgba(234, 179, 8, 0.1);
    border: 1px solid rgba(234, 179, 8, 0.35);
    border-radius: 8px;
    padding: 0.85rem 1rem;
    font-size: 0.88rem;
    color: #fde68a;
    line-height: 1.5;
  }

  .setup-step-title {
    font-size: 0.95rem;
    font-weight: 700;
    color: #f0f6fc;
    margin: 0;
  }

  .setup-text {
    margin: 0;
    font-size: 0.88rem;
    color: #c9d1d9;
    line-height: 1.5;
  }

  .setup-text code {
    background: rgba(99, 102, 241, 0.15);
    color: #c7d2fe;
    padding: 2px 5px;
    border-radius: 4px;
    font-size: 0.85em;
  }

  .setup-hint {
    margin: 0;
    font-size: 0.82rem;
    color: #6e7681;
    line-height: 1.4;
  }

  .setup-hint code {
    background: rgba(255, 255, 255, 0.06);
    padding: 1px 4px;
    border-radius: 3px;
  }

  .download-link {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    background: rgba(34, 197, 94, 0.1);
    border: 1px solid rgba(34, 197, 94, 0.3);
    color: #86efac;
    padding: 0.6rem 1rem;
    border-radius: 8px;
    font-size: 0.85rem;
    font-weight: 600;
    text-decoration: none;
    transition: background 0.15s;
    width: fit-content;
  }

  .download-link:hover {
    background: rgba(34, 197, 94, 0.18);
    color: #a7f3d0;
  }

  .path-box {
    display: flex;
    align-items: stretch;
    border: 1px solid #30363d;
    border-radius: 8px;
    overflow: hidden;
    background: #0d1117;
  }

  .path-text {
    flex: 1;
    padding: 0.65rem 0.85rem;
    font-size: 0.78rem;
    color: #c7d2fe;
    word-break: break-all;
    line-height: 1.5;
    font-family: 'Cascadia Code', 'Fira Code', monospace;
  }

  .copy-btn {
    background: rgba(99, 102, 241, 0.15);
    border: none;
    border-left: 1px solid #30363d;
    color: #c7d2fe;
    padding: 0 0.85rem;
    cursor: pointer;
    font-size: 0.8rem;
    font-weight: 600;
    white-space: nowrap;
    transition: background 0.15s;
    font-family: inherit;
  }

  .copy-btn:hover {
    background: rgba(99, 102, 241, 0.28);
  }

  .setup-requirements {
    background: #0d1117;
    border: 1px solid #30363d;
    border-radius: 8px;
    padding: 0.85rem 1rem;
    font-size: 0.85rem;
    color: #c9d1d9;
  }

  .setup-requirements strong {
    color: #f0f6fc;
    display: block;
    margin-bottom: 0.5rem;
  }

  .setup-requirements ul {
    margin: 0;
    padding-left: 1.25rem;
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }

  /* Help modal */
  .help-body {
    gap: 0;
  }

  .help-section {
    padding: 1rem 0;
    border-bottom: 1px solid rgba(48, 54, 61, 0.6);
  }

  .help-section:last-child {
    border-bottom: none;
  }

  .help-section-title {
    font-size: 0.95rem;
    font-weight: 700;
    color: #f0f6fc;
    margin: 0 0 0.6rem 0;
  }

  .help-section p {
    margin: 0 0 0.6rem 0;
    font-size: 0.88rem;
    color: #c9d1d9;
    line-height: 1.55;
  }

  .help-section ul {
    margin: 0 0 0.5rem 0;
    padding-left: 1.25rem;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.88rem;
    color: #c9d1d9;
  }

  .help-section code {
    background: rgba(99, 102, 241, 0.15);
    color: #c7d2fe;
    padding: 1px 5px;
    border-radius: 4px;
    font-size: 0.85em;
  }

  .help-section kbd {
    background: #21262d;
    color: #f0f6fc;
    border: 1px solid #484f58;
    border-radius: 4px;
    padding: 1px 6px;
    font-size: 0.85em;
    font-weight: 600;
  }

  .help-tip {
    background: rgba(99, 102, 241, 0.1);
    border: 1px solid rgba(99, 102, 241, 0.25);
    border-radius: 6px;
    padding: 0.6rem 0.85rem;
    font-size: 0.84rem;
    color: #c7d2fe;
    margin-top: 0.4rem;
  }

  .code-example {
    background: #0d1117;
    border: 1px solid #30363d;
    border-radius: 6px;
    padding: 0.65rem 0.85rem;
    margin: 0.3rem 0;
  }

  .code-example code {
    background: none;
    color: #86efac;
    padding: 0;
    font-size: 0.85rem;
  }

  .shortcuts-table {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .shortcut-row {
    display: flex;
    align-items: center;
    gap: 1rem;
    font-size: 0.88rem;
    color: #c9d1d9;
  }

  .shortcut-row kbd {
    background: #21262d;
    color: #f0f6fc;
    border: 1px solid #484f58;
    border-radius: 4px;
    padding: 2px 7px;
    font-size: 0.82rem;
    font-weight: 600;
    white-space: nowrap;
  }

  .shortcut-row span {
    color: #8b949e;
    font-size: 0.85rem;
  }
</style>
