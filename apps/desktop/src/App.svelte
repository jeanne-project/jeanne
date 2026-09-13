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

    if (label !== 'quick-access') {
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
</script>

{#if isQuickAccess}
  <QuickAccess />
{:else}
  <main class="main-dashboard">
    <header class="header">
      <div class="brand-row">
        <h1 class="logo-title">Jeanne</h1>
        <span class="version-badge">v{coreVersion}</span>
      </div>
      <p class="tagline">Local-first AI knowledge assistant and meeting co-pilot.</p>
    </header>

    <div class="stats-grid">
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

    <section class="overlay-info">
      <h2>Palette d'Accès Rapide</h2>
      <p>Activez la palette flottante à tout instant via le raccourci global :</p>
      <div class="shortcut-box">
        <kbd>Alt</kbd> + <kbd>Espace</kbd>
      </div>
      <p class="hint">Repli automatique sur <kbd>Alt</kbd> + <kbd>Maj</kbd> + <kbd>Espace</kbd> si déjà réservé par le système.</p>
    </section>

    <section class="preview-section">
      <h3>Aperçu interactif de la Palette</h3>
      <div class="preview-wrapper">
        <QuickAccess />
      </div>
    </section>
  </main>
{/if}

<style>
  :global(body) {
    background-color: #0d1117;
    color: #e6edf3;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
  }

  .main-dashboard {
    max-width: 860px;
    margin: 0 auto;
    padding: 2.5rem 1.5rem;
  }

  .header {
    margin-bottom: 2rem;
  }

  .brand-row {
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
    background: rgba(99, 102, 241, 0.15);
    color: #a5b4fc;
    font-size: 0.8rem;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: 6px;
    border: 1px solid rgba(99, 102, 241, 0.25);
  }

  .tagline {
    color: #8b949e;
    margin-top: 0.4rem;
    font-size: 1.05rem;
  }

  .stats-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: 1rem;
    margin-bottom: 2.5rem;
  }

  .stat-card {
    background: rgba(22, 27, 34, 0.75);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 10px;
    padding: 1.25rem;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .stat-label {
    font-size: 0.82rem;
    color: #8b949e;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .stat-value {
    font-size: 1.6rem;
    font-weight: 700;
    color: #f0f6fc;
  }

  .overlay-info {
    background: rgba(22, 27, 34, 0.5);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 12px;
    padding: 1.5rem;
    margin-bottom: 2.5rem;
  }

  .overlay-info h2 {
    font-size: 1.25rem;
    margin-top: 0;
    margin-bottom: 0.5rem;
  }

  .shortcut-box {
    display: inline-block;
    padding: 0.6rem 1.2rem;
    background: rgba(99, 102, 241, 0.12);
    border: 1px solid rgba(99, 102, 241, 0.3);
    border-radius: 8px;
    font-size: 1.1rem;
    margin: 0.6rem 0;
  }

  kbd {
    background: rgba(255, 255, 255, 0.12);
    border: 1px solid rgba(255, 255, 255, 0.2);
    border-radius: 4px;
    padding: 2px 7px;
    font-family: inherit;
    font-size: 0.9em;
  }

  .hint {
    font-size: 0.85rem;
    color: #8b949e;
    margin-top: 0.5rem;
  }

  .preview-section {
    margin-top: 2rem;
  }

  .preview-section h3 {
    font-size: 1.1rem;
    color: #94a3b8;
    margin-bottom: 1rem;
  }

  .preview-wrapper {
    background: rgba(13, 17, 23, 0.9);
    border: 1px dashed rgba(255, 255, 255, 0.15);
    border-radius: 12px;
    padding: 1.5rem;
  }
</style>
