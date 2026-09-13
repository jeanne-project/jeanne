<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { LogicalSize } from '@tauri-apps/api/dpi';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import type { SearchResult } from '../types/ipc';

  let query = $state('');
  let results = $state<SearchResult[]>([]);
  let selectedIndex = $state(0);
  let isLoading = $state(false);
  let statusMessage = $state<string | null>(null);
  let inputElement = $state<HTMLInputElement | null>(null);

  const isNoteCommand = $derived(query.trim().startsWith('/note'));
  const hasResults = $derived(results.length > 0);

  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  const currentWindow = getCurrentWebviewWindow();
  let isQuickAccessWindow = false;
  try {
    isQuickAccessWindow = currentWindow.label === 'quick-access';
  } catch {
    isQuickAccessWindow = false;
  }

  async function adjustWindowSize(targetHeight: number) {
    if (!isQuickAccessWindow) return;
    try {
      await invoke('set_quick_access_height', { height: targetHeight });
    } catch {
      try {
        await currentWindow.setSize(new LogicalSize(720, targetHeight));
      } catch {
        // Ignorer si l'API de redimensionnement n'est pas accessible en mode test
      }
    }
  }

  async function hideWindow() {
    if (!isQuickAccessWindow) return;
    try {
      await currentWindow.hide();
    } catch {
      // Ignorer si non disponible
    }
  }

  async function executeSearch(searchQuery: string) {
    const trimmed = searchQuery.trim();
    if (!trimmed) {
      results = [];
      selectedIndex = 0;
      await adjustWindowSize(84);
      return;
    }

    if (trimmed.startsWith('/note')) {
      results = [];
      selectedIndex = 0;
      await adjustWindowSize(115);
      return;
    }

    isLoading = true;
    try {
      const searchHits = await invoke<SearchResult[]>('search_notes', {
        query: trimmed,
        limit: 8,
      });
      results = searchHits;
      selectedIndex = 0;
      await adjustWindowSize(searchHits.length > 0 ? 400 : 84);
    } catch (err) {
      statusMessage = `Erreur recherche : ${err}`;
      results = [];
      await adjustWindowSize(110);
    } finally {
      isLoading = false;
    }
  }

  // Déclenchement de la recherche avec debouncing sans mutation synchrone de state dans l'effect
  $effect(() => {
    const currentQ = query;
    if (debounceTimer) {
      clearTimeout(debounceTimer);
    }

    const delay = currentQ.trim().startsWith('/note') ? 0 : 120;
    debounceTimer = setTimeout(() => {
      executeSearch(currentQ);
    }, delay);

    return () => {
      if (debounceTimer) clearTimeout(debounceTimer);
    };
  });

  async function handleSelectResult(item: SearchResult) {
    try {
      await invoke('open_note_in_editor', { filePath: item.file_path });
      await hideWindow();
    } catch (err) {
      statusMessage = `Impossible d'ouvrir : ${err}`;
    }
  }

  async function handleCaptureNote() {
    const trimmed = query.trim();
    const noteBody = trimmed.startsWith('/note') ? trimmed.slice(5).trim() : trimmed;
    if (!noteBody) {
      statusMessage = 'Le contenu de la note ne peut pas être vide';
      return;
    }

    try {
      isLoading = true;
      statusMessage = 'Enregistrement de la note...';
      const path = await invoke<string>('capture_quick_note', { content: trimmed });
      statusMessage = `Note enregistrée dans ${path}`;
      query = '';
      results = [];
      setTimeout(async () => {
        statusMessage = null;
        await hideWindow();
      }, 400);
    } catch (err) {
      statusMessage = `Erreur capture : ${err}`;
    } finally {
      isLoading = false;
    }
  }

  function handleKeyDown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      hideWindow();
      return;
    }

    if (event.key === 'ArrowDown') {
      event.preventDefault();
      if (results.length > 0) {
        selectedIndex = (selectedIndex + 1) % results.length;
      }
      return;
    }

    if (event.key === 'ArrowUp') {
      event.preventDefault();
      if (results.length > 0) {
        selectedIndex = (selectedIndex - 1 + results.length) % results.length;
      }
      return;
    }

    if (event.key === 'Enter') {
      event.preventDefault();
      if (isNoteCommand) {
        handleCaptureNote();
      } else if (results.length > 0) {
        handleSelectResult(results[selectedIndex]);
      }
    }
  }

  // Gestionnaire de cycle de vie et d'écouteurs d'événements Tauri 100% Runes
  $effect(() => {
    inputElement?.focus();

    if (!isQuickAccessWindow) {
      return;
    }

    let isMounted = true;
    let unlistenBlur: (() => void) | undefined;
    let unlistenFocus: (() => void) | undefined;

    (async () => {
      try {
        const uBlur = await currentWindow.listen('tauri://blur', () => {
          hideWindow();
        });
        if (!isMounted) {
          uBlur();
        } else {
          unlistenBlur = uBlur;
        }

        const uFocus = await currentWindow.listen('tauri://focus', () => {
          inputElement?.focus();
          inputElement?.select();
        });
        if (!isMounted) {
          uFocus();
        } else {
          unlistenFocus = uFocus;
        }
      } catch {
        // Mode hors Tauri (tests/navigateur)
      }
    })();

    return () => {
      isMounted = false;
      if (unlistenBlur) unlistenBlur();
      if (unlistenFocus) unlistenFocus();
    };
  });
</script>

<div class="overlay-container" onkeydown={handleKeyDown} role="dialog" aria-label="Palette d'accès rapide" tabindex="-1">
  <div class="palette-card">
    <div class="search-bar">
      <div class="search-icon" aria-hidden="true">
        {#if isLoading}
          <div class="spinner"></div>
        {:else if isNoteCommand}
          <span class="command-badge">NOTE</span>
        {:else}
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="11" cy="11" r="8"></circle>
            <line x1="21" y1="21" x2="16.65" y2="16.65"></line>
          </svg>
        {/if}
      </div>

      <!-- svelte-ignore a11y_autofocus -->
      <input
        bind:this={inputElement}
        bind:value={query}
        type="text"
        placeholder="Rechercher une note ou taper '/note [texte]' pour consigner..."
        autocomplete="off"
        spellcheck="false"
        autofocus
      />

      {#if query}
        <button class="clear-button" onclick={() => { query = ''; inputElement?.focus(); }} aria-label="Effacer la saisie">
          ✕
        </button>
      {/if}
    </div>

    {#if isNoteCommand}
      <div class="quick-note-preview">
        <span class="badge-tag">Action</span>
        <span class="preview-text">Appuyez sur <kbd>Entrée</kbd> pour horodater dans le <strong>Journal quotidien</strong>.</span>
      </div>
    {/if}

    {#if hasResults}
      <div class="results-list" role="listbox">
        {#each results as item, index}
          <button
            type="button"
            class="result-item"
            class:selected={index === selectedIndex}
            onclick={() => handleSelectResult(item)}
            onmouseenter={() => { selectedIndex = index; }}
            role="option"
            aria-selected={index === selectedIndex}
            tabindex="-1"
          >
            <div class="result-header">
              <span class="result-title">{item.title}</span>
              <span class="result-path">{item.file_path}</span>
            </div>
            <div class="result-snippet">
              <!-- eslint-disable-next-line svelte/no-at-html-tags -->
              {@html item.snippet}
            </div>
          </button>
        {/each}
      </div>
    {/if}

    {#if statusMessage}
      <div class="status-banner">
        {statusMessage}
      </div>
    {/if}

    <div class="palette-footer">
      <div class="shortcut-hints">
        <span><kbd>↑</kbd><kbd>↓</kbd> Naviguer</span>
        <span><kbd>↵</kbd> {isNoteCommand ? 'Enregistrer' : 'Ouvrir'}</span>
        <span><kbd>Échap</kbd> Fermer</span>
      </div>
      <div class="brand">Jeanne Core v1</div>
    </div>
  </div>
</div>

<style>
  :global(body, html) {
    margin: 0;
    padding: 0;
    background: transparent !important;
    overflow: hidden;
    user-select: none;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif;
  }

  .overlay-container {
    width: 100vw;
    height: 100vh;
    box-sizing: border-box;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding: 0;
    background: transparent;
  }

  .palette-card {
    width: 100%;
    max-width: 720px;
    background: rgba(18, 22, 31, 0.88);
    backdrop-filter: blur(24px);
    -webkit-backdrop-filter: blur(24px);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 14px;
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.55), 0 0 0 1px rgba(255, 255, 255, 0.05);
    overflow: hidden;
    display: flex;
    flex-direction: column;
    color: #e2e8f0;
  }

  .search-bar {
    display: flex;
    align-items: center;
    padding: 0.65rem 1rem;
    gap: 0.75rem;
    box-sizing: border-box;
    background: rgba(255, 255, 255, 0.03);
    border-bottom: 1px solid rgba(255, 255, 255, 0.06);
  }

  .search-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    color: #94a3b8;
  }

  .command-badge {
    background: linear-gradient(135deg, #6366f1, #8b5cf6);
    color: #ffffff;
    font-size: 0.65rem;
    font-weight: 700;
    padding: 2px 6px;
    border-radius: 4px;
    letter-spacing: 0.05em;
  }

  .spinner {
    width: 16px;
    height: 16px;
    border: 2px solid rgba(255, 255, 255, 0.2);
    border-top-color: #6366f1;
    border-radius: 50%;
    animation: spin 0.6s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    font-size: 1.1rem;
    color: #f8fafc;
    font-weight: 400;
  }

  input::placeholder {
    color: #64748b;
    font-size: 1rem;
  }

  .clear-button {
    background: transparent;
    border: none;
    color: #64748b;
    cursor: pointer;
    font-size: 0.9rem;
    padding: 4px 8px;
    border-radius: 4px;
    transition: color 0.15s ease;
  }

  .clear-button:hover {
    color: #cbd5e1;
  }

  .quick-note-preview {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.6rem 1rem;
    background: rgba(99, 102, 241, 0.1);
    border-bottom: 1px solid rgba(99, 102, 241, 0.2);
    font-size: 0.85rem;
    color: #c7d2fe;
  }

  .badge-tag {
    background: #6366f1;
    color: #fff;
    font-size: 0.7rem;
    font-weight: 600;
    padding: 1px 6px;
    border-radius: 4px;
  }

  .results-list {
    max-height: 270px;
    overflow-y: auto;
    padding: 0.4rem;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .results-list::-webkit-scrollbar {
    width: 5px;
  }

  .results-list::-webkit-scrollbar-thumb {
    background: rgba(255, 255, 255, 0.15);
    border-radius: 3px;
  }

  .result-item {
    padding: 0.6rem 0.8rem;
    border-radius: 8px;
    cursor: pointer;
    border: none;
    text-align: left;
    font-family: inherit;
    width: 100%;
    display: block;
    box-sizing: border-box;
    background: transparent;
    transition: background 0.12s ease, transform 0.08s ease;
  }

  .result-item:hover,
  .result-item.selected {
    background: rgba(99, 102, 241, 0.22);
    box-shadow: inset 0 0 0 1px rgba(129, 140, 248, 0.35);
  }

  .result-header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    margin-bottom: 0.2rem;
  }

  .result-title {
    font-weight: 600;
    font-size: 0.95rem;
    color: #f1f5f9;
  }

  .result-path {
    font-size: 0.75rem;
    color: #94a3b8;
    font-family: monospace;
  }

  .result-snippet {
    font-size: 0.82rem;
    color: #cbd5e1;
    line-height: 1.35;
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    line-clamp: 2;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
  }

  :global(.result-snippet mark) {
    background: rgba(251, 191, 36, 0.35);
    color: #fef08a;
    font-weight: 600;
    padding: 1px 3px;
    border-radius: 2px;
  }

  .status-banner {
    padding: 0.4rem 1rem;
    font-size: 0.8rem;
    background: rgba(34, 197, 94, 0.15);
    color: #86efac;
    border-top: 1px solid rgba(34, 197, 94, 0.2);
  }

  .palette-footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.35rem 1rem;
    box-sizing: border-box;
    background: rgba(0, 0, 0, 0.25);
    border-top: 1px solid rgba(255, 255, 255, 0.05);
    font-size: 0.72rem;
    color: #64748b;
  }

  .shortcut-hints {
    display: flex;
    gap: 0.85rem;
  }

  kbd {
    background: rgba(255, 255, 255, 0.1);
    color: #cbd5e1;
    padding: 1px 5px;
    border-radius: 3px;
    font-family: inherit;
    font-size: 0.7rem;
    margin-right: 3px;
    border: 1px solid rgba(255, 255, 255, 0.12);
  }

  .brand {
    font-weight: 600;
    letter-spacing: 0.05em;
    color: #475569;
  }
</style>
