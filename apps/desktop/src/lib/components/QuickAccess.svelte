<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { LogicalSize } from '@tauri-apps/api/dpi';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import type { SearchResult, TaskItem, SnippetItem } from '../types/ipc';

  interface Props {
    mode?: 'overlay' | 'embedded';
  }

  let { mode = 'overlay' }: Props = $props();

  let query = $state('');
  let results = $state<SearchResult[]>([]);
  let selectedIndex = $state(0);
  let isLoading = $state(false);
  let statusMessage = $state<string | null>(null);
  let inputElement = $state<HTMLInputElement | null>(null);

  // États pour les fonctionnalités étendues
  let mathResult = $state<number | null>(null);
  let tasksList = $state<TaskItem[]>([]);
  let snippetsList = $state<SnippetItem[]>([]);
  let scratchpadText = $state('');
  let aiOutput = $state<string | null>(null);

  // Minuteur actif
  let timerActive = $state(false);
  let timerSecondsLeft = $state(0);
  let timerLabel = $state('');
  let timerInterval: ReturnType<typeof setInterval> | null = null;

  // Historique local du presse-papier
  let clipboardHistory = $state<string[]>([]);

  const COMMANDS = [
    { cmd: '/note', label: 'Note rapide', desc: 'Horodater une note dans le Journal', icon: '📝' },
    { cmd: '/todo', label: 'Tâche', desc: 'Ajouter une tâche à Inbox.md', icon: '✅' },
    { cmd: '/tasks', label: 'Mes tâches', desc: 'Voir et cocher les tâches en cours', icon: '📋' },
    { cmd: '/log', label: 'Micro-journal', desc: 'Ajouter une entrée horodatée au Journal', icon: '⏱️' },
    { cmd: '/meeting', label: 'Réunion', desc: 'Créer une fiche de réunion structurée', icon: '🤝' },
    { cmd: '/bookmark', label: 'Signet', desc: 'Enregistrer une URL dans Bookmarks.md', icon: '🔖' },
    { cmd: '/snip', label: 'Snippets', desc: 'Insérer un modèle de texte réutilisable', icon: '✂️' },
    { cmd: '/timer', label: 'Minuteur', desc: 'Lancer un compte à rebours (ex: /timer 25m Pause)', icon: '⏳' },
    { cmd: '/clip', label: 'Presse-papier', desc: 'Historique des copies récentes', icon: '📎' },
    { cmd: '/scratch', label: 'Brouillon', desc: 'Bloc-notes éphémère', icon: '📄' },
    { cmd: '/corrige', label: 'IA Relecture', desc: 'Corriger orthographe/syntaxe du presse-papier', icon: '✨' },
    { cmd: '/rephrase', label: 'IA Reformulation', desc: 'Reformuler le presse-papier (pro, court)', icon: '🔄' },
    { cmd: '/tldr', label: 'IA Résumé', desc: 'Résumer le presse-papier en 3 puces clés', icon: '⚡' },
    { cmd: '/trad', label: 'IA Traduction', desc: 'Traduire le presse-papier (ex: /trad anglais)', icon: '🌐' },
    { cmd: '/ask', label: 'IA Questions', desc: 'Poser une question à votre coffre (RAG)', icon: '🧠' },
  ];

  const currentWindow = getCurrentWebviewWindow();
  let isQuickAccessWindow = false;
  try {
    isQuickAccessWindow = currentWindow.label === 'quick-access';
  } catch {
    isQuickAccessWindow = false;
  }

  const isEmbedded = $derived(mode === 'embedded' || !isQuickAccessWindow);

  // Commandes détectées
  const activeCommand = $derived.by(() => {
    const trimmed = query.trim();
    if (!trimmed.startsWith('/')) return null;
    const firstWord = trimmed.split(' ')[0].toLowerCase();
    return COMMANDS.find((c) => c.cmd === firstWord) ?? null;
  });

  const matchingCommandSuggestions = $derived.by(() => {
    const trimmed = query.trim();
    if (!trimmed.startsWith('/') || activeCommand) return [];
    return COMMANDS.filter((c) => c.cmd.startsWith(trimmed.toLowerCase()));
  });

  const isMathQuery = $derived(mathResult !== null);
  const isTasksView = $derived(activeCommand?.cmd === '/tasks');
  const isSnippetsView = $derived(activeCommand?.cmd === '/snip');
  const isScratchView = $derived(activeCommand?.cmd === '/scratch');
  const isClipView = $derived(activeCommand?.cmd === '/clip');

  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  async function adjustWindowSize(targetHeight: number) {
    if (isEmbedded) return;
    try {
      await invoke('set_quick_access_height', { height: targetHeight });
    } catch {
      try {
        await currentWindow.setSize(new LogicalSize(720, targetHeight));
      } catch {
        // Ignorer si l'API n'est pas accessible
      }
    }
  }

  async function hideWindow() {
    if (isEmbedded) return;
    try {
      await currentWindow.hide();
    } catch {
      // Ignorer si non disponible
    }
  }

  // Détection arithmétique inline
  function checkMath(input: string) {
    const clean = input.trim();
    if (/^[\d\s+\-*/^%().,]+$/.test(clean) && /[+\-*/^%]/.test(clean) && /\d/.test(clean)) {
      invoke<number>('evaluate_math', { expression: clean })
        .then((res) => {
          mathResult = typeof res === 'number' ? Math.round(res * 1e12) / 1e12 : res;
        })
        .catch(() => {
          mathResult = null;
        });
    } else {
      mathResult = null;
    }
  }

  async function executeSearch(searchQuery: string) {
    const trimmed = searchQuery.trim();
    checkMath(trimmed);

    if (!trimmed) {
      results = [];
      selectedIndex = 0;
      aiOutput = null;
      await adjustWindowSize(84);
      return;
    }

    if (trimmed.startsWith('/')) {
      results = [];
      selectedIndex = 0;

      if (isTasksView) {
        try {
          tasksList = await invoke<TaskItem[]>('get_vault_tasks', { limit: 20 });
          await adjustWindowSize(tasksList.length > 0 ? 380 : 120);
        } catch {
          tasksList = [];
        }
        return;
      }

      if (isSnippetsView) {
        try {
          snippetsList = await invoke<SnippetItem[]>('get_snippets');
          await adjustWindowSize(380);
        } catch {
          snippetsList = [];
        }
        return;
      }

      if (isScratchView) {
        await adjustWindowSize(320);
        return;
      }

      if (isClipView) {
        await adjustWindowSize(clipboardHistory.length > 0 ? 320 : 120);
        return;
      }

      if (matchingCommandSuggestions.length > 0) {
        await adjustWindowSize(Math.min(380, 84 + matchingCommandSuggestions.length * 48));
        return;
      }

      await adjustWindowSize(130);
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
      await adjustWindowSize(searchHits.length > 0 ? 420 : 110);
    } catch (err) {
      statusMessage = `Erreur recherche : ${err}`;
      results = [];
      await adjustWindowSize(110);
    } finally {
      isLoading = false;
    }
  }

  // Écouteur de debouncing sur query
  $effect(() => {
    const currentQ = query;
    if (debounceTimer) clearTimeout(debounceTimer);

    const delay = currentQ.trim().startsWith('/') ? 40 : 120;
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
      if (!isEmbedded) {
        await hideWindow();
      } else {
        notifyUser(`Note ouverte : ${item.title || item.file_path}`);
      }
    } catch (err) {
      statusMessage = `Impossible d'ouvrir : ${err}`;
    }
  }

  function notifyUser(msg: string) {
    statusMessage = msg;
    setTimeout(() => {
      if (statusMessage === msg) statusMessage = null;
    }, 3000);
  }

  async function copyToClipboard(text: string, notification?: string) {
    try {
      await navigator.clipboard.writeText(text);
      if (!clipboardHistory.includes(text)) {
        clipboardHistory = [text, ...clipboardHistory.slice(0, 14)];
      }
      notifyUser(notification ?? 'Copié dans le presse-papier !');
    } catch {
      notifyUser('Échec de copie');
    }
  }

  // Exécution des commandes slash
  async function handleExecuteCommand() {
    const trimmed = query.trim();
    if (!trimmed) return;

    if (mathResult !== null) {
      await copyToClipboard(String(mathResult), `Résultat ${mathResult} copié !`);
      query = '';
      mathResult = null;
      if (!isEmbedded) await hideWindow();
      return;
    }

    const parts = trimmed.split(' ');
    const cmd = parts[0].toLowerCase();
    const arg = parts.slice(1).join(' ').trim();

    try {
      isLoading = true;

      switch (cmd) {
        case '/note': {
          if (!arg) throw new Error('Contenu de note requis');
          const path = await invoke<string>('capture_quick_note', { content: trimmed });
          notifyUser(`Note ajoutée au Journal : ${path}`);
          query = '';
          if (!isEmbedded) setTimeout(hideWindow, 500);
          break;
        }

        case '/todo': {
          if (!arg) throw new Error('Description de la tâche requise');
          const path = await invoke<string>('execute_todo', { content: arg });
          notifyUser(`Tâche ajoutée à Inbox.md : ${arg}`);
          query = '';
          if (!isEmbedded) setTimeout(hideWindow, 500);
          break;
        }

        case '/log': {
          if (!arg) throw new Error('Texte de journal requis');
          await invoke<string>('execute_log', { content: arg });
          notifyUser(`Entrée horodatée ajoutée au Journal du jour`);
          query = '';
          if (!isEmbedded) setTimeout(hideWindow, 500);
          break;
        }

        case '/meeting': {
          if (!arg) throw new Error('Titre de réunion requis');
          const file = await invoke<string>('execute_meeting', { title: arg });
          notifyUser(`Fiche de réunion créée : ${file}`);
          await invoke('open_note_in_editor', { filePath: file });
          query = '';
          if (!isEmbedded) setTimeout(hideWindow, 500);
          break;
        }

        case '/bookmark': {
          if (!arg) throw new Error('URL du signet requise');
          const urlParts = arg.split(' ');
          const url = urlParts[0];
          const comment = urlParts.slice(1).join(' ');
          await invoke<string>('execute_bookmark', { url, comment: comment || undefined });
          notifyUser(`Signet enregistré dans Bookmarks.md`);
          query = '';
          if (!isEmbedded) setTimeout(hideWindow, 500);
          break;
        }

        case '/timer': {
          if (!arg) throw new Error('Durée requise (ex: /timer 25m Pause)');
          const timerParts = arg.split(' ');
          const durStr = timerParts[0].toLowerCase();
          const label = timerParts.slice(1).join(' ') || 'Minuteur';

          let secs = 0;
          if (durStr.endsWith('m')) secs = parseInt(durStr) * 60;
          else if (durStr.endsWith('s')) secs = parseInt(durStr);
          else if (durStr.endsWith('h')) secs = parseInt(durStr) * 3600;
          else secs = parseInt(durStr) * 60;

          if (isNaN(secs) || secs <= 0) throw new Error('Durée invalide (ex: 25m, 10s, 1h)');

          startTimer(secs, label);
          notifyUser(`Minuteur lancé : ${label} (${secs}s)`);
          query = '';
          break;
        }

        case '/corrige':
        case '/rephrase':
        case '/tldr':
        case '/resume':
        case '/trad': {
          let text = '';
          try {
            text = await navigator.clipboard.readText();
          } catch {
            throw new Error('Impossible de lire le presse-papier');
          }

          if (!text.trim()) throw new Error('Le presse-papier est vide');

          const action = cmd.replace('/', '');
          console.debug(`[QuickAccess:AI] Action '${action}' déclenchée sur le presse-papier (taille=${text.length} cars) avec param='${arg}'`);
          const res = await invoke<string>('ai_process_clipboard', {
            action,
            text,
            param: arg || undefined,
          });

          console.info(`[QuickAccess:AI] Réponse reçue (${res.length} cars) :`, res);
          aiOutput = res;
          await copyToClipboard(res, `Résultat IA copié dans le presse-papier !`);
          notifyUser(`IA (${action}) : Résultat généré et copié !`);
          break;
        }

        case '/ask': {
          if (!arg) throw new Error('Question requise');
          console.debug(`[QuickAccess:RAG] Question posée au coffre : '${arg}'`);
          const answer = await invoke<string>('ask_vault', { question: arg });
          console.info(`[QuickAccess:RAG] Réponse reçue (${answer.length} cars) :`, answer);
          aiOutput = answer;
          await adjustWindowSize(400);
          break;
        }

        default:
          if (results.length > 0) {
            await handleSelectResult(results[selectedIndex]);
          }
          break;
      }
    } catch (err: unknown) {
      console.warn(`[QuickAccess] Erreur lors de l'exécution de '${cmd}' :`, err);
      statusMessage = `Erreur : ${String(err)}`;
    } finally {
      isLoading = false;
    }
  }

  function startTimer(seconds: number, label: string) {
    if (timerInterval) clearInterval(timerInterval);
    timerSecondsLeft = seconds;
    timerLabel = label;
    timerActive = true;

    timerInterval = setInterval(() => {
      timerSecondsLeft -= 1;
      if (timerSecondsLeft <= 0) {
        clearInterval(timerInterval!);
        timerActive = false;
        notifyUser(`⏰ Minuteur terminé : ${timerLabel} !`);
        try {
          // Bip audio standard Web Audio
          const ctx = new AudioContext();
          const osc = ctx.createOscillator();
          osc.connect(ctx.destination);
          osc.frequency.value = 880;
          osc.start();
          setTimeout(() => { osc.stop(); }, 500);
        } catch {
          // Ignoré si audio bloqué
        }
      }
    }, 1000);
  }

  async function toggleTask(task: TaskItem) {
    try {
      await invoke('toggle_vault_task', {
        filePath: task.file_path,
        lineNumber: task.line_number,
        checked: !task.checked,
      });
      task.checked = !task.checked;
      tasksList = [...tasksList];
      notifyUser(task.checked ? 'Tâche terminée !' : 'Tâche rouverte');
    } catch (err) {
      notifyUser(`Erreur mise à jour : ${err}`);
    }
  }

  function handleKeyDown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      if (!isEmbedded) {
        hideWindow();
      } else {
        query = '';
        results = [];
        mathResult = null;
        aiOutput = null;
        statusMessage = null;
      }
      return;
    }

    if (event.key === 'ArrowDown') {
      event.preventDefault();
      if (matchingCommandSuggestions.length > 0) {
        selectedIndex = (selectedIndex + 1) % matchingCommandSuggestions.length;
      } else if (results.length > 0) {
        selectedIndex = (selectedIndex + 1) % results.length;
      }
      return;
    }

    if (event.key === 'ArrowUp') {
      event.preventDefault();
      if (matchingCommandSuggestions.length > 0) {
        selectedIndex = (selectedIndex - 1 + matchingCommandSuggestions.length) % matchingCommandSuggestions.length;
      } else if (results.length > 0) {
        selectedIndex = (selectedIndex - 1 + results.length) % results.length;
      }
      return;
    }

    if (event.key === 'Enter') {
      event.preventDefault();
      if (matchingCommandSuggestions.length > 0 && selectedIndex < matchingCommandSuggestions.length) {
        query = matchingCommandSuggestions[selectedIndex].cmd + ' ';
        return;
      }
      handleExecuteCommand();
    }
  }

  $effect(() => {
    if (!isEmbedded) {
      inputElement?.focus();
    }
  });
</script>

<div
  class="search-container"
  class:is-overlay={!isEmbedded}
  class:is-embedded={isEmbedded}
  onkeydown={handleKeyDown}
  role="dialog"
  aria-label="Palette d'accès rapide"
  tabindex="-1"
>
  <div class="palette-card" class:card-embedded={isEmbedded}>
    <!-- Barre de recherche et saisie -->
    <div class="search-bar">
      <div class="search-icon" aria-hidden="true">
        {#if isLoading}
          <div class="spinner"></div>
        {:else if activeCommand}
          <span class="command-badge">{activeCommand.cmd.slice(1).toUpperCase()}</span>
        {:else if isMathQuery}
          <span class="command-badge math-badge">= CALC</span>
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
        placeholder="Rechercher, taper '/' pour les commandes, ou une formule (ex: 12 * 4.5)..."
        autocomplete="off"
        spellcheck="false"
        autofocus={!isEmbedded}
      />

      {#if timerActive}
        <div class="timer-chip" title="Minuteur en cours">
          ⏳ {Math.floor(timerSecondsLeft / 60)}:{String(timerSecondsLeft % 60).padStart(2, '0')}
        </div>
      {/if}

      {#if query}
        <button class="clear-button" onclick={() => { query = ''; mathResult = null; aiOutput = null; inputElement?.focus(); }} aria-label="Effacer la saisie">
          ✕
        </button>
      {/if}
    </div>

    <!-- 1. Aperçu Calculatrice Inline -->
    {#if mathResult !== null}
      <div class="math-preview">
        <span class="math-expr">{query.trim()}</span>
        <span class="math-eq">=</span>
        <strong class="math-val">{mathResult}</strong>
        <span class="math-hint"><kbd>Entrée</kbd> pour copier</span>
      </div>
    {/if}

    <!-- 2. Suggestions de Commandes Slash -->
    {#if matchingCommandSuggestions.length > 0}
      <div class="suggestions-list" role="listbox">
        {#each matchingCommandSuggestions as item, idx}
          <button
            type="button"
            class="suggestion-item"
            class:selected={idx === selectedIndex}
            onclick={() => { query = item.cmd + ' '; inputElement?.focus(); }}
          >
            <span class="sug-icon">{item.icon}</span>
            <strong class="sug-cmd">{item.cmd}</strong>
            <span class="sug-label">{item.label}</span>
            <span class="sug-desc">— {item.desc}</span>
          </button>
        {/each}
      </div>
    {/if}

    <!-- 3. Vue Mes Tâches (/tasks) -->
    {#if isTasksView}
      <div class="tasks-container">
        <div class="sub-header">
          <span>📋 Tâches en cours ({tasksList.filter(t => !t.checked).length})</span>
          <span class="hint-small">Cliquer pour cocher/décocher</span>
        </div>
        {#if tasksList.length === 0}
          <div class="empty-state">Aucune tâche en attente dans Inbox.md ou le Journal du jour ! 🎉</div>
        {:else}
          <div class="tasks-scroll">
            {#each tasksList as task}
              <button type="button" class="task-row" class:task-done={task.checked} onclick={() => toggleTask(task)}>
                <input type="checkbox" checked={task.checked} readonly />
                <span class="task-text">{task.content}</span>
                <span class="task-file">{task.file_path.split('/').pop()}</span>
              </button>
            {/each}
          </div>
        {/if}
      </div>
    {/if}

    <!-- 4. Vue Snippets (/snip) -->
    {#if isSnippetsView}
      <div class="snippets-container">
        <div class="sub-header">
          <span>✂️ Modèles et Snippets de texte</span>
          <span class="hint-small">Cliquer pour copier dans le presse-papier</span>
        </div>
        <div class="snippets-scroll">
          {#each snippetsList as snip}
            <button type="button" class="snip-card" onclick={() => copyToClipboard(snip.content, `Snippet "${snip.title}" copié !`)}>
              <strong class="snip-title">{snip.title}</strong>
              <code class="snip-preview">{snip.content.slice(0, 100)}...</code>
            </button>
          {/each}
        </div>
      </div>
    {/if}

    <!-- 5. Vue Bloc-Notes Brouillon (/scratch) -->
    {#if isScratchView}
      <div class="scratch-container">
        <div class="sub-header">
          <span>📄 Brouillon éphémère</span>
          <div class="scratch-actions">
            <button class="btn-xs" onclick={() => copyToClipboard(scratchpadText)}>Copier</button>
            <button class="btn-xs" onclick={() => { invoke('capture_quick_note', { content: `/note ${scratchpadText}` }); notifyUser('Brouillon enregistré au Journal'); }}>Consigner au Journal</button>
            <button class="btn-xs btn-danger" onclick={() => { scratchpadText = ''; }}>Effacer</button>
          </div>
        </div>
        <textarea
          bind:value={scratchpadText}
          placeholder="Tapez vos notes éphémères ici (numéro, idée rapide...)..."
          rows="6"
        ></textarea>
      </div>
    {/if}

    <!-- 6. Vue Historique Presse-papier (/clip) -->
    {#if isClipView}
      <div class="clip-container">
        <div class="sub-header">
          <span>📎 Historique des copies récentes</span>
        </div>
        {#if clipboardHistory.length === 0}
          <div class="empty-state">L'historique se remplira au fil de vos copies.</div>
        {:else}
          <div class="clip-scroll">
            {#each clipboardHistory as item}
              <button type="button" class="clip-row" onclick={() => copyToClipboard(item)}>
                <span class="clip-text">{item}</span>
                <span class="clip-badge">Copier</span>
              </button>
            {/each}
          </div>
        {/if}
      </div>
    {/if}

    <!-- 7. Réponse IA (/corrige, /ask, /tldr...) -->
    {#if aiOutput}
      <div class="ai-output-box">
        <div class="ai-output-header">
          <span>🤖 Résultat IA</span>
          <button class="btn-xs" onclick={() => copyToClipboard(aiOutput ?? '')}>Copier</button>
        </div>
        <div class="ai-output-content">{aiOutput}</div>
      </div>
    {/if}

    <!-- 8. Résultats de recherche de notes standard -->
    {#if results.length > 0 && !activeCommand}
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

    <!-- Bandeau de statut -->
    {#if statusMessage}
      <div class="status-banner">
        {statusMessage}
      </div>
    {/if}

    <!-- Pied de palette -->
    <div class="palette-footer">
      <div class="shortcut-hints">
        <span><kbd>↑</kbd><kbd>↓</kbd> Naviguer</span>
        <span><kbd>↵</kbd> Valider / Ouvrir</span>
        <span><kbd>Échap</kbd> {isEmbedded ? 'Effacer' : 'Fermer'}</span>
      </div>
      <div class="brand">Jeanne Core — Suite de Productivité</div>
    </div>
  </div>
</div>

<style>
  .search-container {
    box-sizing: border-box;
    display: flex;
    justify-content: center;
    align-items: flex-start;
  }

  .search-container.is-overlay {
    width: 100vw;
    height: 100vh;
    padding: 0;
    background: transparent;
  }

  .search-container.is-embedded {
    width: 100%;
    max-width: 720px;
    margin: 0 auto;
    padding: 0;
  }

  .palette-card {
    width: 100%;
    max-width: 720px;
    background: rgba(18, 22, 31, 0.96);
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

  .palette-card.card-embedded {
    background: #161b22;
    border: 1px solid #30363d;
    box-shadow: 0 8px 30px rgba(0, 0, 0, 0.4);
    transition: border-color 0.15s ease, box-shadow 0.15s ease;
  }

  .palette-card.card-embedded:focus-within {
    border-color: rgba(99, 102, 241, 0.6);
    box-shadow: 0 8px 30px rgba(0, 0, 0, 0.5), 0 0 0 1px rgba(99, 102, 241, 0.4);
  }

  .search-bar {
    display: flex;
    align-items: center;
    padding: 0.75rem 1.1rem;
    gap: 0.75rem;
    box-sizing: border-box;
    background: rgba(255, 255, 255, 0.02);
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
    padding: 2px 7px;
    border-radius: 4px;
    letter-spacing: 0.05em;
  }

  .math-badge {
    background: linear-gradient(135deg, #10b981, #059669);
  }

  .timer-chip {
    background: rgba(245, 158, 11, 0.18);
    color: #fbbf24;
    border: 1px solid rgba(245, 158, 11, 0.35);
    font-size: 0.75rem;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: 12px;
    white-space: nowrap;
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
    font-size: 1.05rem;
    color: #f8fafc;
    font-weight: 400;
  }

  input::placeholder {
    color: #64748b;
    font-size: 0.95rem;
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

  /* Calculatrice Inline */
  .math-preview {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.75rem 1.25rem;
    background: rgba(16, 185, 129, 0.12);
    border-bottom: 1px solid rgba(16, 185, 129, 0.25);
    font-size: 1.05rem;
    color: #d1fae5;
  }

  .math-eq {
    color: #6ee7b7;
    font-weight: 600;
  }

  .math-val {
    font-size: 1.3rem;
    color: #34d399;
  }

  .math-hint {
    margin-left: auto;
    font-size: 0.78rem;
    color: #a7f3d0;
  }

  /* Suggestions de commandes */
  .suggestions-list {
    max-height: 280px;
    overflow-y: auto;
    padding: 0.4rem;
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }

  .suggestion-item,
  .task-row,
  .snip-card,
  .clip-row {
    font-family: inherit;
    border: none;
    text-align: left;
    width: 100%;
    box-sizing: border-box;
    color: inherit;
    background: transparent;
  }

  .suggestion-item {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.5rem 0.8rem;
    border-radius: 8px;
    cursor: pointer;
    transition: background 0.12s ease;
  }

  .suggestion-item:hover,
  .suggestion-item.selected {
    background: rgba(99, 102, 241, 0.2);
  }

  .sug-icon {
    font-size: 1rem;
  }

  .sug-cmd {
    color: #a5b4fc;
    font-family: monospace;
    font-size: 0.9rem;
  }

  .sug-label {
    font-weight: 600;
    font-size: 0.88rem;
    color: #f1f5f9;
  }

  .sug-desc {
    color: #94a3b8;
    font-size: 0.82rem;
  }

  /* Tâches */
  .tasks-container,
  .snippets-container,
  .scratch-container,
  .clip-container {
    padding: 0.8rem 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    max-height: 280px;
  }

  .sub-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 0.82rem;
    font-weight: 600;
    color: #94a3b8;
    margin-bottom: 0.25rem;
  }

  .hint-small {
    font-size: 0.75rem;
    color: #64748b;
  }

  .tasks-scroll,
  .snippets-scroll,
  .clip-scroll {
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .task-row {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.45rem 0.6rem;
    background: rgba(255, 255, 255, 0.03);
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.88rem;
  }

  .task-row:hover {
    background: rgba(255, 255, 255, 0.06);
  }

  .task-done {
    opacity: 0.5;
    text-decoration: line-through;
  }

  .task-text {
    flex: 1;
    color: #f1f5f9;
  }

  .task-file {
    font-size: 0.72rem;
    color: #64748b;
    font-family: monospace;
  }

  /* Snippets */
  .snip-card {
    padding: 0.5rem 0.75rem;
    background: rgba(255, 255, 255, 0.03);
    border-radius: 6px;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }

  .snip-card:hover {
    background: rgba(99, 102, 241, 0.18);
  }

  .snip-title {
    font-size: 0.88rem;
    color: #c7d2fe;
  }

  .snip-preview {
    font-size: 0.78rem;
    color: #94a3b8;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Brouillon / Scratchpad */
  textarea {
    width: 100%;
    box-sizing: border-box;
    background: #0d1117;
    border: 1px solid #30363d;
    border-radius: 6px;
    color: #f0f6fc;
    padding: 0.6rem;
    font-family: inherit;
    font-size: 0.88rem;
    resize: none;
    outline: none;
  }

  textarea:focus {
    border-color: #6366f1;
  }

  .scratch-actions {
    display: flex;
    gap: 0.4rem;
  }

  .btn-xs {
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.12);
    color: #e2e8f0;
    padding: 2px 7px;
    border-radius: 4px;
    font-size: 0.75rem;
    cursor: pointer;
  }

  .btn-xs:hover {
    background: rgba(255, 255, 255, 0.15);
  }

  .btn-danger {
    color: #fca5a5;
    background: rgba(239, 68, 68, 0.12);
  }

  /* Presse-papier */
  .clip-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.45rem 0.6rem;
    background: rgba(255, 255, 255, 0.03);
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.84rem;
  }

  .clip-row:hover {
    background: rgba(99, 102, 241, 0.18);
  }

  .clip-text {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    color: #e2e8f0;
    max-width: 85%;
  }

  .clip-badge {
    font-size: 0.7rem;
    color: #818cf8;
  }

  /* Sortie IA */
  .ai-output-box {
    padding: 0.8rem 1.1rem;
    background: rgba(99, 102, 241, 0.12);
    border-bottom: 1px solid rgba(99, 102, 241, 0.25);
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .ai-output-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 0.8rem;
    font-weight: 700;
    color: #c7d2fe;
  }

  .ai-output-content {
    font-size: 0.9rem;
    line-height: 1.45;
    color: #f8fafc;
    white-space: pre-wrap;
    max-height: 200px;
    overflow-y: auto;
  }

  .empty-state {
    padding: 1rem;
    text-align: center;
    font-size: 0.85rem;
    color: #64748b;
  }

  /* Résultats de recherche standards */
  .results-list {
    max-height: 280px;
    overflow-y: auto;
    padding: 0.4rem;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
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
    transition: background 0.12s ease;
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
    padding: 0.5rem 1rem;
    font-size: 0.8rem;
    background: rgba(34, 197, 94, 0.15);
    color: #86efac;
    border-top: 1px solid rgba(34, 197, 94, 0.2);
  }

  .palette-footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.4rem 1rem;
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
