<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import QuickAccess from './lib/components/QuickAccess.svelte';
  import type { VaultStats } from './lib/types/ipc';

  let windowLabel = $state('main');
  let coreVersion = $state('Chargement...');
  let vaultStats = $state<VaultStats | null>(null);

  const isQuickAccess = $derived(windowLabel === 'quick-access');

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

  .quit-button {
    display: inline-flex;
    align-items: center;
    gap: 0.45rem;
    background: rgba(239, 68, 68, 0.12);
    border: 1px solid rgba(239, 68, 68, 0.3);
    color: #fca5a5;
    padding: 0.45rem 0.85rem;
    border-radius: 8px;
    font-size: 0.82rem;
    font-weight: 600;
    cursor: pointer;
    font-family: inherit;
    transition: background 0.15s ease, border-color 0.15s ease, color 0.15s ease;
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
    font-size: 1.35rem;
    font-weight: 700;
    color: #f0f6fc;
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
</style>
