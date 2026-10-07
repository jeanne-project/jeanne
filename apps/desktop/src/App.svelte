<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import QuickAccess from './lib/components/QuickAccess.svelte';
  import type { VaultStats, HardwareInfo, LocalInferenceStats, DiscoveredModel, LocalEngineConfig, ModelRecommendedParams, VoiceStatus, AudioDevicesReport, VoiceConfig } from './lib/types/ipc';

  let windowLabel = $state('main');
  let coreVersion = $state('Chargement...');
  let vaultStats = $state<VaultStats | null>(null);
  let hardwareInfo = $state<HardwareInfo | null>(null);
  let inferenceStats = $state<LocalInferenceStats | null>(null);
  let isModelLoading = $state(false);

  // Pipeline Vocal (Jalon 5 / INC-07)
  let voiceStatus = $state<VoiceStatus | null>(null);
  let isVoiceToggling = $state(false);
  let audioDevices = $state<AudioDevicesReport | null>(null);
  let voiceConfig = $state<VoiceConfig>({
    whisper_model_path: null,
    piper_model_path: null,
    selected_input_device: null,
    selected_output_device: null,
    remote_stt_endpoint: null,
  });
  let isSavingVoiceConfig = $state(false);
  let voiceConfigNotice = $state('');

  // Paramètres & Découverte Multi-Modèles
  let showSettingsModal = $state(false);
  let availableModels = $state<DiscoveredModel[]>([]);
  let modelsDirectory = $state('');
  let selectedModelPath = $state('');
  let isScanningModels = $state(false);
  let modelLoadMessage = $state('');
  let modelLoadError = $state('');
  let modelsDirCopied = $state(false);
  let dismissedRecommendedModelPath = $state<string | null>(null);
  let recommendedSuccessNotice = $state('');

  // Configuration Moteur Local & Inférence Avancée
  let engineConfig = $state<LocalEngineConfig>({
    context_size: 4096,
    threads: null,
    use_vulkan: true,
    use_gpu: true,
    gpu_layers: null,
    generation_timeout_secs: 10,
    temperature: 0.3,
    top_p: 0.8,
    top_k: 20,
    max_tokens: 1024,
    allow_extended_context: false,
    daemon_endpoint: null,
    daemon_api_key: null,
    daemon_model: null,
  });
  let configSavedMessage = $state('');
  let isSavingConfig = $state(false);

  // Serveur d'Inférence Personnalisé & Découverte Modèles
  let remoteModelsList = $state<string[]>([]);
  let isFetchingRemoteModels = $state(false);
  let remoteModelsFetchMessage = $state('');
  let remoteModelsFetchError = $state('');
  let showApiKey = $state(false);
  const isCustomServerConfigured = $derived(
    Boolean(engineConfig.daemon_endpoint && engineConfig.daemon_endpoint.trim() !== '')
  );

  // Modals Aide & Setup
  let showHelpModal = $state(false);
  let showModelSetupModal = $state(false);
  let modelSetupPath = $state('');
  let modelSetupError = $state('');
  let pathCopied = $state(false);

  const isQuickAccess = $derived(windowLabel === 'quick-access');
  const loadedModel = $derived(availableModels.find((m) => m.is_loaded));
  const activeModelForRecommendation = $derived(
    loadedModel || (selectedModelPath ? availableModels.find((m) => m.path === selectedModelPath) : null)
  );

  function hasRecommendedDifferences(model: DiscoveredModel | null | undefined): boolean {
    if (!model || !model.recommended_params) return false;
    const rec = model.recommended_params;
    if (rec.context_size && rec.context_size !== engineConfig.context_size) return true;
    if (rec.temperature !== undefined && rec.temperature !== null && Math.abs(rec.temperature - engineConfig.temperature) > 0.01) return true;
    if (rec.top_p !== undefined && rec.top_p !== null && Math.abs(rec.top_p - (engineConfig.top_p ?? 0.8)) > 0.01) return true;
    if (rec.top_k !== undefined && rec.top_k !== null && rec.top_k !== (engineConfig.top_k ?? 20)) return true;
    return false;
  }

  async function applyRecommendedParams(model: DiscoveredModel) {
    if (!model.recommended_params) return;
    const rec = model.recommended_params;
    if (rec.context_size) {
      if (rec.context_size > 4096) {
        engineConfig.allow_extended_context = true;
      }
      engineConfig.context_size = rec.context_size;
    }
    if (rec.temperature !== undefined && rec.temperature !== null) {
      engineConfig.temperature = rec.temperature;
    }
    if (rec.top_p !== undefined && rec.top_p !== null) {
      engineConfig.top_p = rec.top_p;
    }
    if (rec.top_k !== undefined && rec.top_k !== null) {
      engineConfig.top_k = rec.top_k;
    }
    await saveEngineConfig();
    recommendedSuccessNotice = `Paramètres recommandés pour ${model.name} appliqués avec succès !`;
    dismissedRecommendedModelPath = model.path;
    setTimeout(() => {
      recommendedSuccessNotice = '';
    }, 4500);
  }

  function dismissRecommendedParams(model: DiscoveredModel) {
    dismissedRecommendedModelPath = model.path;
  }

  async function refreshHardware() {
    try {
      hardwareInfo = await invoke<HardwareInfo>('get_hardware_profile');
      inferenceStats = await invoke<LocalInferenceStats>('get_local_inference_stats');
    } catch {
      // Ignoré si mode web pur
    }
  }

  async function refreshVoice() {
    try {
      voiceStatus = await invoke<VoiceStatus>('get_voice_status');
      audioDevices = await invoke<AudioDevicesReport>('list_audio_devices');
      const cfg = await invoke<VoiceConfig>('get_voice_config');
      if (cfg) {
        voiceConfig = { ...cfg };
      }
    } catch {
      // Ignoré si mode web pur
    }
  }

  async function saveVoiceConfig() {
    isSavingVoiceConfig = true;
    voiceConfigNotice = '';
    try {
      const updated = await invoke<VoiceConfig>('update_voice_config', { config: voiceConfig });
      voiceConfig = { ...updated };
      voiceConfigNotice = '✓ Paramètres vocaux enregistrés avec succès !';
      setTimeout(() => {
        voiceConfigNotice = '';
      }, 4000);
    } catch (e) {
      console.error('Erreur enregistrement paramètres vocaux', e);
      voiceConfigNotice = `Erreur : ${String(e)}`;
    } finally {
      isSavingVoiceConfig = false;
    }
  }

  async function handleToggleVoice() {
    if (isVoiceToggling) return;
    isVoiceToggling = true;
    try {
      const target = !(voiceStatus?.is_active);
      await invoke<boolean>('toggle_voice_pipeline', { active: target });
      await refreshVoice();
    } catch (e) {
      console.error('Erreur bascule vocal:', e);
    } finally {
      isVoiceToggling = false;
    }
  }

  async function loadModelsList() {
    isScanningModels = true;
    modelLoadError = '';
    try {
      modelsDirectory = await invoke<string>('get_models_directory');
      availableModels = await invoke<DiscoveredModel[]>('list_available_models');
      const loaded = availableModels.find((m) => m.is_loaded);
      if (loaded) {
        selectedModelPath = loaded.path;
      } else if (!selectedModelPath && availableModels.length > 0) {
        selectedModelPath = availableModels[0].path;
      }
    } catch (e) {
      console.error('Erreur scan modèles', e);
    } finally {
      isScanningModels = false;
    }
  }

  async function handleLoadSelectedModel(modelPath?: string) {
    const targetPath = modelPath || selectedModelPath;
    isModelLoading = true;
    modelLoadMessage = '';
    modelLoadError = '';
    try {
      await invoke('load_local_model', { modelPath: targetPath || null });
      await refreshHardware();
      await loadModelsList();
      dismissedRecommendedModelPath = null;
      modelLoadMessage = 'Modèle chargé avec succès en mémoire vive !';
      setTimeout(() => {
        modelLoadMessage = '';
      }, 4000);
    } catch (e: unknown) {
      modelLoadError = String(e);
    } finally {
      isModelLoading = false;
    }
  }

  async function handleUnloadModel() {
    isModelLoading = true;
    modelLoadMessage = '';
    modelLoadError = '';
    try {
      await invoke('unload_local_model');
      await refreshHardware();
      await loadModelsList();
      dismissedRecommendedModelPath = null;
      modelLoadMessage = 'Modèle déchargé avec succès (< 200 Mo RAM).';
      setTimeout(() => {
        modelLoadMessage = '';
      }, 4000);
    } catch (e: unknown) {
      modelLoadError = String(e);
    } finally {
      isModelLoading = false;
    }
  }

  async function loadEngineConfig() {
    try {
      const cfg = await invoke<LocalEngineConfig>('get_local_engine_config');
      engineConfig = cfg;
      if (cfg.daemon_model && remoteModelsList.length === 0) {
        remoteModelsList = [cfg.daemon_model];
      }
    } catch (e) {
      console.error('Erreur chargement config moteur', e);
    }
  }

  async function handleFetchRemoteModels() {
    if (!engineConfig.daemon_endpoint || !engineConfig.daemon_endpoint.trim()) {
      remoteModelsFetchError = "Veuillez renseigner l'URL du serveur d'inférence (ex: http://127.0.0.1:11434/v1).";
      return;
    }
    isFetchingRemoteModels = true;
    remoteModelsFetchMessage = '';
    remoteModelsFetchError = '';
    try {
      const models = await invoke<string[]>('fetch_remote_server_models', {
        endpoint: engineConfig.daemon_endpoint.trim(),
        apiKey:
          engineConfig.daemon_api_key && engineConfig.daemon_api_key.trim() !== ''
            ? engineConfig.daemon_api_key.trim()
            : null,
      });
      remoteModelsList = models;
      if (models.length > 0) {
        if (!engineConfig.daemon_model || !models.includes(engineConfig.daemon_model)) {
          engineConfig.daemon_model = models[0];
        }
        remoteModelsFetchMessage = `✓ ${models.length} modèle(s) détecté(s) sur le serveur !`;
        await saveEngineConfig();
      } else {
        remoteModelsFetchMessage = 'Serveur accessible, mais aucun modèle retourné dans la liste.';
      }
      setTimeout(() => {
        remoteModelsFetchMessage = '';
      }, 5000);
    } catch (e) {
      console.error('Erreur récupération modèles distants:', e);
      remoteModelsFetchError = `Erreur : ${String(e)}`;
    } finally {
      isFetchingRemoteModels = false;
    }
  }

  async function saveEngineConfig() {
    isSavingConfig = true;
    configSavedMessage = '';
    try {
      const isPowerfulOrExtended = Boolean(
        engineConfig.allow_extended_context ||
        (hardwareInfo && hardwareInfo.total_system_ram_mb > 16384)
      );
      const maxAllowedContext = isPowerfulOrExtended ? 32768 : 4096;
      const requestedContext = Number(engineConfig.context_size) || 4096;
      const boundedContext = Math.min(maxAllowedContext, Math.max(512, requestedContext));

      const payload: LocalEngineConfig = {
        ...engineConfig,
        generation_timeout_secs: Math.max(1, Number(engineConfig.generation_timeout_secs) || 10),
        temperature: Math.min(2.0, Math.max(0.0, Number(engineConfig.temperature) || 0.3)),
        top_p:
          engineConfig.top_p !== null && engineConfig.top_p !== undefined
            ? Math.min(1.0, Math.max(0.0, Number(engineConfig.top_p)))
            : 0.8,
        top_k:
          engineConfig.top_k !== null && engineConfig.top_k !== undefined
            ? Math.max(1, Math.min(200, Number(engineConfig.top_k)))
            : 20,
        max_tokens: Math.max(64, Number(engineConfig.max_tokens) || 1024),
        context_size: boundedContext,
        allow_extended_context: Boolean(engineConfig.allow_extended_context),
        threads: engineConfig.threads ? Number(engineConfig.threads) : null,
        gpu_layers:
          engineConfig.gpu_layers !== null &&
          engineConfig.gpu_layers !== undefined &&
          String(engineConfig.gpu_layers) !== ''
            ? Number(engineConfig.gpu_layers)
            : null,
        daemon_endpoint:
          engineConfig.daemon_endpoint && engineConfig.daemon_endpoint.trim() !== ''
            ? engineConfig.daemon_endpoint.trim()
            : null,
        daemon_api_key:
          engineConfig.daemon_api_key && engineConfig.daemon_api_key.trim() !== ''
            ? engineConfig.daemon_api_key.trim()
            : null,
        daemon_model:
          engineConfig.daemon_model && engineConfig.daemon_model.trim() !== ''
            ? engineConfig.daemon_model.trim()
            : null,
      };
      const updated = await invoke<LocalEngineConfig>('update_local_engine_config', { config: payload });
      engineConfig = updated;
      configSavedMessage = 'Paramètres d’inférence enregistrés avec succès !';
      await refreshHardware();
      setTimeout(() => {
        configSavedMessage = '';
      }, 3500);
    } catch (e) {
      console.error('Erreur sauvegarde config', e);
      modelLoadError = `Erreur enregistrement : ${String(e)}`;
    } finally {
      isSavingConfig = false;
    }
  }

  function resetEngineConfig() {
    engineConfig = {
      model_path: null,
      context_size: 4096,
      threads: null,
      use_vulkan: true,
      use_gpu: true,
      gpu_layers: null,
      generation_timeout_secs: 10,
      temperature: 0.3,
      top_p: 0.8,
      top_k: 20,
      max_tokens: 1024,
      allow_extended_context: false,
      daemon_endpoint: null,
      daemon_api_key: null,
      daemon_model: null,
      expected_sha256: null,
    };
    saveEngineConfig();
  }

  function openSettings() {
    showSettingsModal = true;
    modelLoadMessage = '';
    modelLoadError = '';
    configSavedMessage = '';
    recommendedSuccessNotice = '';
    voiceConfigNotice = '';
    loadModelsList();
    loadEngineConfig();
    refreshHardware();
    refreshVoice();
  }

  async function copyModelsDirectory() {
    try {
      await navigator.clipboard.writeText(modelsDirectory);
      modelsDirCopied = true;
      setTimeout(() => {
        modelsDirCopied = false;
      }, 2000);
    } catch {
      // Fallback si clipboard non disponible
    }
  }

  async function toggleLocalModel() {
    if (!hardwareInfo) return;
    isModelLoading = true;
    try {
      if (hardwareInfo.recommended_model_loaded) {
        await invoke('unload_local_model');
      } else {
        await invoke('load_local_model', { modelPath: selectedModelPath || null });
      }
      await refreshHardware();
      await loadModelsList();
    } catch (e: unknown) {
      const errMsg = String(e);
      // Si le modèle est introuvable → afficher le guide d'installation
      if (errMsg.includes('does not exist') || errMsg.includes('not found') || errMsg.includes('No such file')) {
        try {
          modelSetupPath = await invoke<string>('get_default_model_path');
        } catch {
          modelSetupPath = '(chemin non disponible)';
        }
        if (isCustomServerConfigured) {
          modelSetupError = '';
        } else {
          modelSetupError = errMsg;
        }
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
      setTimeout(() => {
        pathCopied = false;
      }, 2000);
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
      refreshVoice();
      loadModelsList();
      loadEngineConfig();
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
      showSettingsModal = false;
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
            class="settings-button"
            onclick={openSettings}
            title="Paramètres de Jeanne (Modèles GGUF, Diagnostic Matériel...)"
            aria-label="Ouvrir les paramètres"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
              <circle cx="12" cy="12" r="3"></circle>
              <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"></path>
            </svg>
            <span>Paramètres</span>
          </button>
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

      <!-- Moteur d'Inférence Local (GGUF / Vulkan) ou Serveur Personnalisé -->
      <section class="model-section" aria-label="Moteur d'inférence">
        <div class="model-header">
          <h2 class="section-title">
            {#if isCustomServerConfigured}
              Serveur d'Inférence Personnalisé
            {:else}
              Inférence Locale (Vulkan / GGUF)
            {/if}
          </h2>
          <span class="status-badge {hardwareInfo?.recommended_model_loaded || isCustomServerConfigured ? 'status-active' : 'status-idle'}">
            {#if isCustomServerConfigured}
              Serveur Actif : {engineConfig.daemon_model || 'Modèle distant'}
            {:else if hardwareInfo?.recommended_model_loaded}
              {loadedModel ? `Modèle Actif : ${loadedModel.name}` : 'Modèle Chargé (3B)'}
            {:else}
              Modèle Déchargé
            {/if}
          </span>
        </div>
        <p class="model-description">
          {#if isCustomServerConfigured}
            Inférence active via <code>{engineConfig.daemon_endpoint}</code> (modèle : <strong>{engineConfig.daemon_model || 'Par défaut'}</strong>). Empreinte RAM minimale (&lt; 150 Mo).
          {:else if loadedModel}
            Modèle en cours d'exécution : <strong>{loadedModel.name}</strong> ({loadedModel.size_formatted}{loadedModel.architecture ? `, ${loadedModel.architecture}` : ''}). Exécution 100% hors-ligne.
          {:else}
            Exécution souveraine 100% hors-ligne. Détecte automatiquement vos modèles dans <code>models/</code>.
          {/if}
        </p>
        <div class="stats-cards">
          <div class="stat-card">
            <span class="stat-label">RAM Système (Dispo / Total)</span>
            <span class="stat-value">{hardwareInfo ? (hardwareInfo.total_system_ram_mb > 0 ? `${Math.round(hardwareInfo.available_ram_mb / 1024)}G / ${Math.round(hardwareInfo.total_system_ram_mb / 1024)}G` : '- / -') : '...'}</span>
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
            {:else if isCustomServerConfigured}
              <span>Charger un Modèle Local (Optionnel)</span>
            {:else}
              <span>Charger le Modèle Local</span>
            {/if}
          </button>
          <button
            type="button"
            class="model-settings-btn"
            onclick={openSettings}
            title="Gérer et sélectionner parmi les modèles GGUF détectés"
          >
            ⚙️ Paramètres Modèles ({availableModels.length > 0 ? `${availableModels.length} détecté${availableModels.length > 1 ? 's' : ''}` : 'Gérer'})
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

      <!-- Pipeline Vocal Bidirectionnel (Jalon 5) -->
      <section class="model-section voice-section" aria-label="Pipeline Vocal Bidirectionnel">
        <div class="model-header">
          <div class="model-title-group">
            <h2 class="section-title">Interaction Vocale Bidirectionnelle</h2>
            <span class="model-subtitle">STT Whisper &amp; TTS Piper streaming avec VAD &amp; rééchantillonnage 16 kHz</span>
          </div>
          <span class="status-badge {voiceStatus?.is_active ? 'status-active' : 'status-idle'}">
            {voiceStatus?.is_active ? `Vocal Actif (${voiceStatus.state})` : 'Vocal Inactif (0 Mo RAM)'}
          </span>
        </div>

        <div class="stats-cards">
          <div class="stat-card">
            <span class="stat-label">Microphone Actif</span>
            <span class="stat-value">{voiceConfig.selected_input_device || audioDevices?.default_input_name || 'Microphone Système'}</span>
          </div>
          <div class="stat-card">
            <span class="stat-label">Sortie Audio TTS</span>
            <span class="stat-value">{voiceConfig.selected_output_device || audioDevices?.default_output_name || 'Haut-parleur Système'}</span>
          </div>
          <div class="stat-card">
            <span class="stat-label">Latence Synthèse (TTFB)</span>
            <span class="stat-value">{voiceStatus?.last_synthesis_ttfb_ms ?? 0} ms (&lt; 800 ms)</span>
          </div>
          <div class="stat-card">
            <span class="stat-label">Empreinte RAM Vocale</span>
            <span class="stat-value">{voiceStatus?.memory_allocated_mb ?? 0} Mo</span>
          </div>
        </div>

        <!-- Paramétrage Audio & Modèles Vocaux (INC-07) -->
        <div class="voice-config-container" style="background: rgba(255,255,255,0.03); border: 1px solid rgba(255,255,255,0.08); border-radius: 8px; padding: 1rem; margin-bottom: 1rem;">
          <h3 style="margin-top: 0; margin-bottom: 0.75rem; font-size: 0.95rem; color: #a5b4fc;">⚙️ Configuration Audio & Modèles Vocaux</h3>
          <div class="adv-grid" style="display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap: 1rem;">
            <div class="adv-field">
              <label for="voice-mic-select" class="adv-label" style="display: block; font-size: 0.85rem; margin-bottom: 0.35rem; color: #cbd5e1;">🎤 Microphone d'entrée :</label>
              <select
                id="voice-mic-select"
                class="adv-select"
                style="width: 100%; padding: 0.45rem 0.6rem; background: #1e293b; color: #f8fafc; border: 1px solid #334155; border-radius: 6px;"
                bind:value={voiceConfig.selected_input_device}
                onchange={saveVoiceConfig}
              >
                <option value={null}>Microphone par défaut ({audioDevices?.default_input_name ?? 'Système'})</option>
                {#if audioDevices?.input_devices}
                  {#each audioDevices.input_devices as dev}
                    <option value={dev.name}>{dev.name} {dev.is_default ? '(par défaut)' : ''}</option>
                  {/each}
                {/if}
              </select>
            </div>

            <div class="adv-field">
              <label for="voice-speaker-select" class="adv-label" style="display: block; font-size: 0.85rem; margin-bottom: 0.35rem; color: #cbd5e1;">🔊 Sortie audio (Haut-parleur) :</label>
              <select
                id="voice-speaker-select"
                class="adv-select"
                style="width: 100%; padding: 0.45rem 0.6rem; background: #1e293b; color: #f8fafc; border: 1px solid #334155; border-radius: 6px;"
                bind:value={voiceConfig.selected_output_device}
                onchange={saveVoiceConfig}
              >
                <option value={null}>Sortie par défaut ({audioDevices?.default_output_name ?? 'Système'})</option>
                {#if audioDevices?.output_devices}
                  {#each audioDevices.output_devices as dev}
                    <option value={dev.name}>{dev.name} {dev.is_default ? '(par défaut)' : ''}</option>
                  {/each}
                {/if}
              </select>
            </div>

            <div class="adv-field">
              <label for="voice-whisper-model" class="adv-label" style="display: block; font-size: 0.85rem; margin-bottom: 0.35rem; color: #cbd5e1;">🧠 Modèle Whisper STT (.bin) :</label>
              <input
                id="voice-whisper-model"
                type="text"
                class="adv-input"
                style="width: 100%; padding: 0.45rem 0.6rem; background: #1e293b; color: #f8fafc; border: 1px solid #334155; border-radius: 6px;"
                placeholder="ex: /path/to/ggml-base.bin (optionnel)"
                bind:value={voiceConfig.whisper_model_path}
                onchange={saveVoiceConfig}
              />
            </div>

            <div class="adv-field">
              <label for="voice-piper-model" class="adv-label" style="display: block; font-size: 0.85rem; margin-bottom: 0.35rem; color: #cbd5e1;">🗣️ Modèle Piper TTS (.onnx) :</label>
              <input
                id="voice-piper-model"
                type="text"
                class="adv-input"
                style="width: 100%; padding: 0.45rem 0.6rem; background: #1e293b; color: #f8fafc; border: 1px solid #334155; border-radius: 6px;"
                placeholder="ex: /path/to/fr_FR-siwis-medium.onnx (optionnel)"
                bind:value={voiceConfig.piper_model_path}
                onchange={saveVoiceConfig}
              />
            </div>
          </div>

          <div style="margin-top: 0.75rem; display: flex; align-items: center; justify-content: space-between;">
            <button
              type="button"
              class="btn-sm btn-primary"
              style="padding: 0.35rem 0.75rem; font-size: 0.85rem;"
              onclick={saveVoiceConfig}
              disabled={isSavingVoiceConfig}
            >
              {isSavingVoiceConfig ? 'Enregistrement...' : '💾 Sauvegarder les paramètres vocaux'}
            </button>
            {#if voiceConfigNotice}
              <span style="font-size: 0.85rem; color: #4ade80;">{voiceConfigNotice}</span>
            {/if}
          </div>
        </div>

        <div class="model-actions">
          <button
            type="button"
            class="model-toggle-btn {voiceStatus?.is_active ? 'btn-unload' : 'btn-load'}"
            onclick={handleToggleVoice}
            disabled={isVoiceToggling}
          >
            {#if isVoiceToggling}
              <span class="spinner"></span>
              <span>Bascule en cours...</span>
            {:else if voiceStatus?.is_active}
              <span>🎙️ Désactiver l'Interaction Vocale (0 Mo)</span>
            {:else}
              <span>🎙️ Activer l'Interaction Vocale</span>
            {/if}
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
        {#if isCustomServerConfigured}
          <div class="alert-box alert-info-server">
            <strong>ℹ️ Serveur d'inférence personnalisé déjà configuré (<code>{engineConfig.daemon_endpoint}</code>).</strong><br/>
            Ce guide de téléchargement n'est requis que si vous désirez une exécution 100% hors-ligne sans serveur.
          </div>
        {/if}
        {#if modelSetupError}
          <div class="alert-box">
            <strong>⚠️ Modèle introuvable</strong><br/>
            Le fichier GGUF n'a pas été trouvé à l'emplacement attendu.
          </div>
        {/if}

        <h3 class="setup-step-title">Étape 1 — Télécharger le modèle</h3>
        <p class="setup-text">Téléchargez le fichier <code>Qwen3.5-2B-Q4_K_M.gguf</code> (~1.28 Go) depuis Hugging Face :</p>
        <a
          class="download-link"
          href="https://huggingface.co/unsloth/Qwen3.5-2B-GGUF/resolve/main/Qwen3.5-2B-Q4_K_M.gguf"
          target="_blank"
          rel="noopener noreferrer"
        >
          🤗 Hugging Face — Qwen3.5-2B-Q4_K_M.gguf
        </a>
        <p class="setup-hint">Connectez-vous sur Hugging Face si demandé. Téléchargement direct (~1.28 Go).</p>

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
        <p class="setup-text">Cliquez sur <strong>« Charger le Modèle Local (Qwen 3.5 2B) »</strong> dans le tableau de bord. Le chargement prend quelques secondes (~1.3 Go en mémoire).</p>

        <div class="setup-requirements">
          <strong>⚙️ Configuration recommandée :</strong>
          <ul>
            <li>RAM : 8 Go disponibles minimum (16 Go total recommandé)</li>
            <li>GPU : Vulkan compatible (Intel Iris Xe, AMD Radeon, NVIDIA GeForce)</li>
            <li>Espace disque : ~1.3 Go pour le fichier GGUF</li>
          </ul>
        </div>
      </div>
      <div class="modal-footer">
        <button class="btn-primary" onclick={() => { showModelSetupModal = false; }}>Compris, je télécharge</button>
      </div>
    </div>
  </div>
{/if}

<!-- ─── Modal : Paramètres de Jeanne & Choix du Modèle Local ─── -->
{#if showSettingsModal}
  <div class="modal-backdrop" onclick={() => { showSettingsModal = false; }} role="presentation">
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="modal modal-wide" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-labelledby="modal-settings-title" tabindex="-1">
      <div class="modal-header">
        <div class="modal-title-row">
          <span class="settings-header-icon">⚙️</span>
          <h2 id="modal-settings-title" class="modal-title">Paramètres & Modèles Locaux</h2>
        </div>
        <button class="modal-close" onclick={() => { showSettingsModal = false; }} aria-label="Fermer">✕</button>
      </div>

      <div class="modal-body settings-body">
        {#if modelLoadMessage}
          <div class="alert-box alert-success">
            <strong>✓ Succès :</strong> {modelLoadMessage}
          </div>
        {/if}
        {#if modelLoadError}
          <div class="alert-box alert-danger">
            <strong>⚠️ Erreur :</strong> {modelLoadError}
          </div>
        {/if}

        <!-- Section 1 : Emplacement des Modèles -->
        <section class="settings-section">
          <div class="section-title-bar">
            <h3 class="settings-subtitle">📁 Répertoire des Modèles GGUF</h3>
            <button class="btn-xs" onclick={loadModelsList} disabled={isScanningModels}>
              {isScanningModels ? '🔄 Analyse en cours...' : '🔄 Actualiser'}
            </button>
          </div>
          <p class="settings-desc">
            Déposez vos fichiers quantifiés <code>.gguf</code> dans ce dossier (ou le dossier <code>models/</code> à la racine du projet). Jeanne les détecte automatiquement.
          </p>
          <div class="path-box">
            <code class="path-text">{modelsDirectory || 'Recherche du répertoire...'}</code>
            <button class="copy-btn" onclick={copyModelsDirectory} title="Copier le chemin">
              {modelsDirCopied ? '✓ Copié !' : '📋 Copier'}
            </button>
          </div>
        </section>

        <!-- Section 2 : Sélection du Modèle Local -->
        <section class="settings-section">
          <div class="section-title-bar">
            <h3 class="settings-subtitle">🤖 Modèles Détectés ({availableModels.length})</h3>
            <span class="badge-count {loadedModel ? 'badge-count-active' : ''}">
              {loadedModel ? `Actif : ${loadedModel.name}` : 'Aucun modèle en mémoire'}
            </span>
          </div>

          {#if availableModels.length === 0}
            {#if isCustomServerConfigured}
              <div class="empty-models-box server-active-box">
                <div class="server-active-icon">🟢</div>
                <div class="server-active-details">
                  <p class="server-active-title"><strong>Inférence assurée par le serveur personnalisé</strong></p>
                  <p class="hint-muted">
                    Votre serveur (<code>{engineConfig.daemon_endpoint}</code>) avec le modèle <strong>{engineConfig.daemon_model || 'par défaut'}</strong> est actif et prêt à l'emploi. Le téléchargement de fichiers <code>.gguf</code> locaux est entièrement facultatif.
                  </p>
                </div>
              </div>
            {:else}
              <div class="empty-models-box">
                <p><strong>⚠️ Aucun modèle <code>.gguf</code> détecté dans le dossier <code>models/</code>.</strong></p>
                <p class="hint-muted">
                  Pour exécuter l'IA en local 100% hors-ligne, déposez un ou plusieurs fichiers <code>.gguf</code> dans le dossier ci-dessus, puis cliquez sur <strong>« Actualiser »</strong>.
                </p>
                <div class="recommended-downloads">
                  <a
                    class="model-download-card"
                    href="https://huggingface.co/unsloth/Qwen3.5-2B-GGUF/resolve/main/Qwen3.5-2B-Q4_K_M.gguf"
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    <div class="dl-info">
                      <span class="dl-title">Qwen3.5-2B (Q4_K_M) — Recommandé</span>
                      <span class="dl-desc">Ultra-frugal, rapide et précis, optimisé pour PC &le; 16 Go avec iGPU (~1.28 Go)</span>
                    </div>
                    <span class="dl-action">🤗 Télécharger</span>
                  </a>
                  <a
                    class="model-download-card"
                    href="https://huggingface.co/bartowski/Llama-3.2-3B-Instruct-GGUF/resolve/main/Llama-3.2-3B-Instruct-Q4_K_M.gguf"
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    <div class="dl-info">
                      <span class="dl-title">Llama-3.2-3B-Instruct (Q4_K_M)</span>
                      <span class="dl-desc">Grande polyvalence en rédaction et synthèse (~2.0 Go)</span>
                    </div>
                    <span class="dl-action">🤗 Télécharger</span>
                  </a>
                </div>
              </div>
            {/if}
          {:else}
            <div class="models-grid">
              {#each availableModels as model}
                <div class="model-card {model.is_loaded ? 'model-card-active' : ''} {!model.fits_ram ? 'model-card-heavy' : ''}">
                  <div class="model-card-header">
                    <div class="model-name-col">
                      <div class="model-title-row">
                        <span class="model-title">{model.name}</span>
                        {#if model.is_loaded}
                          <span class="tag tag-loaded">🟢 Actif en mémoire</span>
                        {/if}
                      </div>
                      <div class="model-tags">
                        <span class="tag tag-size">💾 {model.size_formatted}</span>
                        {#if model.architecture}
                          <span class="tag tag-arch">🏛️ {model.architecture}</span>
                        {/if}
                        {#if model.context_length}
                          <span class="tag tag-ctx">📏 {model.context_length >= 1024 ? `${Math.round(model.context_length / 1024)}K` : model.context_length} tok</span>
                        {/if}
                        {#if model.recommended_params}
                          <span class="tag tag-rec" title="Paramètres recommandés disponibles">✨ Profil optimal</span>
                        {/if}
                        {#if !model.fits_ram}
                          <span class="tag tag-warning">⚠️ &gt; 4.5 Go (Vérifier RAM)</span>
                        {:else}
                          <span class="tag tag-ok">✓ Compatible RAM</span>
                        {/if}
                      </div>
                      <div class="model-path-hint" title={model.path}>
                        <code>{model.path}</code>
                      </div>
                    </div>
                    <div class="model-card-actions">
                      {#if model.is_loaded}
                        <button
                          type="button"
                          class="btn-sm btn-unload"
                          onclick={handleUnloadModel}
                          disabled={isModelLoading}
                        >
                          Décharger (&lt; 200 Mo)
                        </button>
                      {:else}
                        <button
                          type="button"
                          class="btn-sm btn-load"
                          onclick={() => handleLoadSelectedModel(model.path)}
                          disabled={isModelLoading}
                        >
                          {isModelLoading ? 'Chargement...' : 'Choisir & Charger'}
                        </button>
                      {/if}
                    </div>
                  </div>
                </div>
              {/each}
            </div>
          {/if}
        </section>

        <!-- Proposition interactive : Paramètres recommandés du modèle GGUF -->
        {#if activeModelForRecommendation && activeModelForRecommendation.recommended_params && hasRecommendedDifferences(activeModelForRecommendation) && dismissedRecommendedModelPath !== activeModelForRecommendation.path}
          <div class="recommendation-card">
            <div class="rec-header">
              <span class="rec-icon">💡</span>
              <div class="rec-header-text">
                <h4 class="rec-title">Paramètres optimaux recommandés pour {activeModelForRecommendation.name}</h4>
                <p class="rec-subtitle">
                  Le fichier GGUF et les spécifications officielles du modèle
                  {#if activeModelForRecommendation.architecture}(architecture <code>{activeModelForRecommendation.architecture}</code>){/if}
                  fournissent des paramètres recommandés différents de votre configuration actuelle :
                </p>
              </div>
            </div>

            <div class="rec-comparison-grid">
              <div class="rec-comparison-item">
                <span class="rec-comp-label">Contexte KV</span>
                <div class="rec-comp-values">
                  <span class="rec-val-current">Actuel : {engineConfig.context_size} tok</span>
                  <span class="rec-arrow">➔</span>
                  <span class="rec-val-recommended">Recommandé : {activeModelForRecommendation.recommended_params.context_size ?? 4096} tok</span>
                </div>
              </div>

              <div class="rec-comparison-item">
                <span class="rec-comp-label">Température</span>
                <div class="rec-comp-values">
                  <span class="rec-val-current">Actuel : {engineConfig.temperature}</span>
                  <span class="rec-arrow">➔</span>
                  <span class="rec-val-recommended">Recommandé : {activeModelForRecommendation.recommended_params.temperature ?? 0.3}</span>
                </div>
              </div>

              <div class="rec-comparison-item">
                <span class="rec-comp-label">Top-P</span>
                <div class="rec-comp-values">
                  <span class="rec-val-current">Actuel : {engineConfig.top_p ?? 0.8}</span>
                  <span class="rec-arrow">➔</span>
                  <span class="rec-val-recommended">Recommandé : {activeModelForRecommendation.recommended_params.top_p ?? 0.8}</span>
                </div>
              </div>

              <div class="rec-comparison-item">
                <span class="rec-comp-label">Top-K</span>
                <div class="rec-comp-values">
                  <span class="rec-val-current">Actuel : {engineConfig.top_k ?? 20}</span>
                  <span class="rec-arrow">➔</span>
                  <span class="rec-val-recommended">Recommandé : {activeModelForRecommendation.recommended_params.top_k ?? 20}</span>
                </div>
              </div>
            </div>

            <div class="rec-actions">
              <button
                type="button"
                class="btn-sm btn-primary rec-btn-apply"
                onclick={() => activeModelForRecommendation && applyRecommendedParams(activeModelForRecommendation)}
              >
                ✓ Remplacer par les paramètres recommandés
              </button>
              <button
                type="button"
                class="btn-sm btn-secondary rec-btn-dismiss"
                onclick={() => activeModelForRecommendation && dismissRecommendedParams(activeModelForRecommendation)}
              >
                Conserver mes paramètres actuels
              </button>
            </div>
          </div>
        {/if}

        {#if recommendedSuccessNotice}
          <div class="alert-box alert-success">
            <strong>✓ Succès :</strong> {recommendedSuccessNotice}
          </div>
        {/if}

        <!-- Section 3 : Inférence Locale & Accélération Matérielle -->
        <section class="settings-section">
          <div class="section-title-bar">
            <h3 class="settings-subtitle">⚡ Inférence Locale & Accélération Matérielle</h3>
            {#if configSavedMessage}
              <span class="save-toast">✓ {configSavedMessage}</span>
            {/if}
          </div>

          <div class="settings-grid">
            <!-- Option 1 : GPU Toggle -->
            <div class="setting-card">
              <div class="setting-header">
                <div class="setting-title-group">
                  <span class="setting-icon">🎮</span>
                  <label for="gpu-toggle" class="setting-label">Accélération Matérielle GPU (Vulkan / DirectML)</label>
                </div>
                <label class="toggle-switch" title="Activer / Désactiver l'utilisation de la carte graphique">
                  <input
                    id="gpu-toggle"
                    type="checkbox"
                    bind:checked={engineConfig.use_gpu}
                    onchange={saveEngineConfig}
                  />
                  <span class="toggle-slider"></span>
                </label>
              </div>
              <p class="setting-desc">
                {#if engineConfig.use_gpu}
                  <strong class="text-success">Actif :</strong> Les calculs sont déchargés sur votre carte graphique ou iGPU partagé pour une vitesse maximale.
                {:else}
                  <strong class="text-warning">Désactivé (Mode CPU forcé) :</strong> Les calculs s'exécutent sur le processeur (RAM système), préservant toute la VRAM pour vos autres applications.
                {/if}
              </p>
            </div>

            <!-- Option 2 : Timeout de génération -->
            <div class="setting-card">
              <div class="setting-header">
                <div class="setting-title-group">
                  <span class="setting-icon">⏱️</span>
                  <label for="timeout-input" class="setting-label">Délai limite de génération (Timeout)</label>
                </div>
                <div class="input-with-unit">
                  <input
                    id="timeout-input"
                    type="number"
                    min="1"
                    max="180"
                    bind:value={engineConfig.generation_timeout_secs}
                    onchange={saveEngineConfig}
                    class="input-number"
                  />
                  <span class="unit-label">sec</span>
                </div>
              </div>
              <p class="setting-desc">
                Interrompt automatiquement la génération si le modèle boucle ou tarde à répondre pour protéger les ressources de votre machine (<strong>10 secondes</strong> par défaut).
              </p>
            </div>
          </div>
        </section>

        <!-- Section 4 : Paramètres Avancés d'Inférence (Repliable) -->
        <section class="settings-section">
          <details class="advanced-accordion">
            <summary class="advanced-summary">
              <span>🔧 Paramètres Avancés d'Inférence (Couches GPU, Threads, Contexte, Température...)</span>
            </summary>
            <div class="advanced-content">
              <div class="advanced-grid">
                <div class="advanced-field">
                  <label for="adv-gpu-layers" class="adv-label">Couches GPU offload (gpu_layers)</label>
                  <input
                    id="adv-gpu-layers"
                    type="number"
                    min="0"
                    max="99"
                    placeholder="Auto (99 couches)"
                    bind:value={engineConfig.gpu_layers}
                    class="adv-input"
                  />
                  <span class="adv-hint">Laisser vide pour tout décharger, ou indiquer le nombre exact de couches.</span>
                </div>

                <div class="advanced-field">
                  <label for="adv-threads" class="adv-label">Cœurs CPU alloués (threads)</label>
                  <input
                    id="adv-threads"
                    type="number"
                    min="1"
                    max="64"
                    placeholder="Auto (cœurs physiques)"
                    bind:value={engineConfig.threads}
                    class="adv-input"
                  />
                  <span class="adv-hint">Nombre de threads de calcul CPU (laisser vide pour détection automatique).</span>
                </div>

                <div class="advanced-field">
                  <label for="adv-context" class="adv-label">Contexte KV maximal (context_size)</label>
                  <select id="adv-context" bind:value={engineConfig.context_size} onchange={saveEngineConfig} class="adv-select">
                    <option value={2048}>2048 jetons (Frugal)</option>
                    <option value={4096}>4096 jetons (Standard, recommandé &le; 16 Go)</option>
                    <option value={8192}>8192 jetons (PC &ge; 24 Go RAM / Contexte étendu)</option>
                    <option value={16384}>16384 jetons (PC &ge; 32 Go RAM / Contexte large)</option>
                    <option value={32768}>32768 jetons (PC puissant / Contexte natif Qwen3.5)</option>
                  </select>
                  <span class="adv-hint">
                    {#if engineConfig.allow_extended_context || (hardwareInfo && hardwareInfo.total_system_ram_mb > 16384)}
                      <span class="text-success">🚀 Mode contexte étendu actif (jusqu'à 32768 tokens).</span>
                    {:else}
                      <span>Strictement borné à 4096 tokens max sur PC &le; 16 Go pour garantir l'empreinte mémoire &lt; 4.5 Go. Débloquez ci-dessous si souhaité.</span>
                    {/if}
                  </span>
                </div>

                <div class="advanced-field">
                  <label for="adv-temp" class="adv-label">Température de créativité (temperature)</label>
                  <input
                    id="adv-temp"
                    type="number"
                    step="0.05"
                    min="0"
                    max="1.5"
                    bind:value={engineConfig.temperature}
                    class="adv-input"
                  />
                  <span class="adv-hint">0.0 = Déterministe/relecture, 0.3 = Idéal assistant Jeanne, 0.8+ = Créatif.</span>
                </div>

                <div class="advanced-field">
                  <label for="adv-top-p" class="adv-label">Filtrage Top-P (top_p)</label>
                  <input
                    id="adv-top-p"
                    type="number"
                    step="0.05"
                    min="0.05"
                    max="1.0"
                    placeholder="0.8"
                    bind:value={engineConfig.top_p}
                    class="adv-input"
                  />
                  <span class="adv-hint">Nucleus sampling (0.8 = standard Qwen, 0.9 = LLaMA, 0.95 = Mistral).</span>
                </div>

                <div class="advanced-field">
                  <label for="adv-top-k" class="adv-label">Filtrage Top-K (top_k)</label>
                  <input
                    id="adv-top-k"
                    type="number"
                    step="1"
                    min="1"
                    max="100"
                    placeholder="20"
                    bind:value={engineConfig.top_k}
                    class="adv-input"
                  />
                  <span class="adv-hint">Nombre de meilleurs jetons considérés (20 = Qwen, 40 = LLaMA).</span>
                </div>

                <div class="advanced-field">
                  <label for="adv-max-tokens" class="adv-label">Jetons max par réponse (max_tokens)</label>
                  <input
                    id="adv-max-tokens"
                    type="number"
                    step="64"
                    min="64"
                    max="4096"
                    bind:value={engineConfig.max_tokens}
                    class="adv-input"
                  />
                  <span class="adv-hint">Longueur maximale de la réponse générée (défaut 1024).</span>
                </div>

                <div class="advanced-field advanced-field-full">
                  <label class="checkbox-label" for="adv-allow-extended">
                    <input
                      id="adv-allow-extended"
                      type="checkbox"
                      bind:checked={engineConfig.allow_extended_context}
                      onchange={saveEngineConfig}
                    />
                    <span>🚀 Débloquer grand contexte (&gt; 4096 jetons) pour PC puissant (&gt; 16 Go RAM / GPU)</span>
                  </label>
                  <span class="adv-hint">
                    Permet d'utiliser des contextes jusqu'à 32768 tokens (Qwen3.5, etc.) pour traiter de longs documents. Attention : un contexte de 32K peut allouer 4 à 8 Go de mémoire supplémentaire pour la table KV d'attention.
                  </span>
                </div>

                <!-- Configuration Complète Serveur d'Inférence Personnalisé -->
                <div class="custom-server-block advanced-field-full">
                  <div class="custom-server-header">
                    <span class="custom-server-title">🌐 Serveur d'Inférence Personnalisé (Ollama, LM Studio, vLLM, API OpenAI)</span>
                    <span class="badge-count {isCustomServerConfigured ? 'badge-count-active' : ''}">
                      {isCustomServerConfigured ? '🟢 Serveur Actif' : 'Non configuré'}
                    </span>
                  </div>
                  <p class="adv-hint">
                    Délègue l'inférence à votre serveur local ou distant. 
                    <strong>Si configuré, le chargement d'un modèle GGUF local n'est plus obligatoire</strong> (empreinte RAM &lt; 150 Mo).
                  </p>

                  <div class="custom-server-inputs-grid">
                    <div class="advanced-field">
                      <label for="adv-daemon" class="adv-label">URL du Serveur d'Inférence (daemon_endpoint)</label>
                      <input
                        id="adv-daemon"
                        type="text"
                        placeholder="ex: http://127.0.0.1:11434/v1 (Ollama) ou http://127.0.0.1:8080/v1"
                        bind:value={engineConfig.daemon_endpoint}
                        onchange={saveEngineConfig}
                        class="adv-input"
                      />
                    </div>

                    <div class="advanced-field">
                      <div class="key-label-row">
                        <label for="adv-daemon-key" class="adv-label">Clé d'API (daemon_api_key)</label>
                        <button
                          type="button"
                          class="key-toggle-btn"
                          onclick={() => (showApiKey = !showApiKey)}
                        >
                          {showApiKey ? '🙈 Masquer' : '👁️ Afficher'}
                        </button>
                      </div>
                      <input
                        id="adv-daemon-key"
                        type={showApiKey ? 'text' : 'password'}
                        placeholder="Optionnel (Bearer token / clé secrète)"
                        bind:value={engineConfig.daemon_api_key}
                        onchange={saveEngineConfig}
                        class="adv-input"
                      />
                    </div>
                  </div>

                  <div class="custom-server-fetch-row">
                    <button
                      type="button"
                      class="btn-sm btn-secondary"
                      onclick={handleFetchRemoteModels}
                      disabled={isFetchingRemoteModels || !engineConfig.daemon_endpoint}
                    >
                      {#if isFetchingRemoteModels}
                        <span class="spinner-sm"></span> Interrogation du serveur...
                      {:else}
                        🔄 Récupérer les modèles disponibles sur le serveur
                      {/if}
                    </button>

                    {#if remoteModelsFetchMessage}
                      <span class="fetch-notice fetch-success">{remoteModelsFetchMessage}</span>
                    {/if}
                    {#if remoteModelsFetchError}
                      <span class="fetch-notice fetch-error">{remoteModelsFetchError}</span>
                    {/if}
                  </div>

                  {#if remoteModelsList.length > 0}
                    <div class="advanced-field" style="margin-top: 10px;">
                      <label for="adv-daemon-model" class="adv-label">Modèle sélectionné sur le serveur</label>
                      <select
                        id="adv-daemon-model"
                        bind:value={engineConfig.daemon_model}
                        onchange={saveEngineConfig}
                        class="adv-select"
                      >
                        {#each remoteModelsList as m}
                          <option value={m}>{m}</option>
                        {/each}
                      </select>
                      <span class="adv-hint">Modèle qui sera automatiquement utilisé pour toutes les requêtes d'inférence et questions au coffre.</span>
                    </div>
                  {:else if isCustomServerConfigured}
                    <div class="advanced-field" style="margin-top: 10px;">
                      <label for="adv-daemon-model-manual" class="adv-label">Nom du modèle (saisie manuelle si /models non exposé)</label>
                      <input
                        id="adv-daemon-model-manual"
                        type="text"
                        placeholder="ex: qwen3.5:2b ou llama-3.2-3b-instruct"
                        bind:value={engineConfig.daemon_model}
                        onchange={saveEngineConfig}
                        class="adv-input"
                      />
                      <span class="adv-hint">Indiquez l'identifiant du modèle actif sur votre serveur.</span>
                    </div>
                  {/if}
                </div>
              </div>

              <div class="advanced-actions">
                <button
                  type="button"
                  class="btn-sm btn-primary"
                  onclick={saveEngineConfig}
                  disabled={isSavingConfig}
                >
                  {isSavingConfig ? 'Enregistrement...' : '💾 Enregistrer les paramètres'}
                </button>
                <button
                  type="button"
                  class="btn-sm btn-secondary"
                  onclick={resetEngineConfig}
                  disabled={isSavingConfig}
                >
                  ↺ Rétablir les valeurs par défaut
                </button>
              </div>
            </div>
          </details>
        </section>

        <!-- Section 4b : Configuration Vocale & Périphériques Audio (INC-07) -->
        <section class="settings-section voice-settings-modal">
          <h3 class="settings-subtitle">🎙️ Configuration Vocale &amp; Périphériques Audio</h3>
          <div class="adv-grid" style="display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap: 1rem; margin-top: 0.75rem;">
            <div class="adv-field">
              <label for="settings-voice-mic" class="adv-label">Microphone d'entrée</label>
              <select
                id="settings-voice-mic"
                class="adv-select"
                bind:value={voiceConfig.selected_input_device}
                onchange={saveVoiceConfig}
              >
                <option value={null}>Microphone par défaut ({audioDevices?.default_input_name ?? 'Système'})</option>
                {#if audioDevices?.input_devices}
                  {#each audioDevices.input_devices as dev}
                    <option value={dev.name}>{dev.name} {dev.is_default ? '(par défaut)' : ''}</option>
                  {/each}
                {/if}
              </select>
              <span class="adv-hint">Périphérique de capture pour la transcription STT.</span>
            </div>

            <div class="adv-field">
              <label for="settings-voice-speaker" class="adv-label">Sortie audio (Haut-parleur)</label>
              <select
                id="settings-voice-speaker"
                class="adv-select"
                bind:value={voiceConfig.selected_output_device}
                onchange={saveVoiceConfig}
              >
                <option value={null}>Sortie par défaut ({audioDevices?.default_output_name ?? 'Système'})</option>
                {#if audioDevices?.output_devices}
                  {#each audioDevices.output_devices as dev}
                    <option value={dev.name}>{dev.name} {dev.is_default ? '(par défaut)' : ''}</option>
                  {/each}
                {/if}
              </select>
              <span class="adv-hint">Périphérique de restitution pour la synthèse vocale TTS.</span>
            </div>

            <div class="adv-field">
              <label for="settings-whisper-model" class="adv-label">Chemin du modèle Whisper (.bin)</label>
              <input
                id="settings-whisper-model"
                type="text"
                class="adv-input"
                placeholder="ex: /path/to/ggml-base.bin (optionnel)"
                bind:value={voiceConfig.whisper_model_path}
                onchange={saveVoiceConfig}
              />
              <span class="adv-hint">Modèle STT Whisper local pour la transcription vocale.</span>
            </div>

            <div class="adv-field">
              <label for="settings-piper-model" class="adv-label">Chemin du modèle Piper (.onnx)</label>
              <input
                id="settings-piper-model"
                type="text"
                class="adv-input"
                placeholder="ex: /path/to/fr_FR-siwis-medium.onnx (optionnel)"
                bind:value={voiceConfig.piper_model_path}
                onchange={saveVoiceConfig}
              />
              <span class="adv-hint">Modèle ONNX pour la synthèse vocale Piper.</span>
            </div>
          </div>

          <div style="margin-top: 0.75rem; display: flex; align-items: center; justify-content: space-between;">
            <button
              type="button"
              class="btn-sm btn-primary"
              onclick={saveVoiceConfig}
              disabled={isSavingVoiceConfig}
            >
              {isSavingVoiceConfig ? 'Enregistrement...' : '💾 Sauvegarder les paramètres vocaux'}
            </button>
            {#if voiceConfigNotice}
              <span style="font-size: 0.85rem; color: #4ade80;">{voiceConfigNotice}</span>
            {/if}
          </div>
        </section>

        <!-- Section 5 : Diagnostic Matériel & Conseils RAM -->
        <section class="settings-section hardware-advice">
          <h3 class="settings-subtitle">⚙️ Diagnostic Matériel & Conseils de Puissance</h3>
          <div class="stats-cards">
            <div class="stat-card">
              <span class="stat-label">RAM Totale / Disponible</span>
              <span class="stat-value">{hardwareInfo ? (hardwareInfo.total_system_ram_mb > 0 ? `${Math.round(hardwareInfo.available_ram_mb / 1024)}G dispo / ${Math.round(hardwareInfo.total_system_ram_mb / 1024)}G` : '- / -') : '...'}</span>
            </div>
            <div class="stat-card">
              <span class="stat-label">Accélération Vulkan (GPU)</span>
              <span class="stat-value">{hardwareInfo?.vulkan_supported ? (hardwareInfo.vulkan_device_name ?? 'Actif') : 'CPU seul'}</span>
            </div>
            <div class="stat-card">
              <span class="stat-label">Contexte KV Conseillé</span>
              <span class="stat-value">{hardwareInfo?.max_recommended_context ?? 4096} tok</span>
            </div>
            <div class="stat-card">
              <span class="stat-label">Plafond Frugal Jeanne</span>
              <span class="stat-value">&lt; 4.5 Go RAM</span>
            </div>
          </div>
          <div class="advice-box">
            <strong>💡 Quel modèle choisir pour votre PC ?</strong>
            <ul>
              <li><strong>PC portable 8 Go ou 16 Go avec iGPU partagé</strong> : Privilégiez impérativement les modèles <strong>2B/3B quantifiés en Q4_K_M</strong> (ex: <code>Qwen3.5-2B</code> ou <code>Llama-3.2-3B</code>). Ils consomment ~1.3 à 2.2 Go de RAM et maintiennent votre système fluide.</li>
              <li><strong>PC 16 Go+ avec GPU dédié ou 32 Go RAM</strong> : Vous pouvez utiliser des modèles <strong>7B</strong> ou <strong>8B</strong> (ex: <code>Qwen2.5-7B-Instruct-Q4_K_M</code>, ~4.4 Go).</li>
            </ul>
          </div>
        </section>
      </div>

      <div class="modal-footer">
        <button class="btn-primary" onclick={() => { showSettingsModal = false; }}>Fermer</button>
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
            <li>Modèle : <code>Qwen3.5-2B-Q4_K_M.gguf</code> (~1.28 Go)</li>
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

  /* ─── Styles Paramètres & Modèles ─── */
  .settings-button {
    background: rgba(14, 165, 233, 0.12);
    border: 1px solid rgba(14, 165, 233, 0.35);
    color: #bae6fd;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 0.45rem;
    padding: 0.45rem 0.85rem;
    border-radius: 8px;
    font-size: 0.82rem;
    font-weight: 600;
    font-family: inherit;
    transition: background 0.15s ease, border-color 0.15s ease, color 0.15s ease;
  }

  .settings-button:hover {
    background: rgba(14, 165, 233, 0.22);
    border-color: rgba(14, 165, 233, 0.6);
    color: #e0f2fe;
  }

  .model-settings-btn {
    padding: 0.55rem 1rem;
    border-radius: 8px;
    font-size: 0.82rem;
    font-weight: 600;
    cursor: pointer;
    font-family: inherit;
    background: rgba(14, 165, 233, 0.12);
    border: 1px solid rgba(14, 165, 233, 0.35);
    color: #7dd3fc;
    transition: all 0.15s ease;
  }

  .model-settings-btn:hover {
    background: rgba(14, 165, 233, 0.22);
    border-color: rgba(14, 165, 233, 0.6);
    color: #bae6fd;
  }

  .modal-title-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .settings-header-icon {
    font-size: 1.25rem;
  }

  .settings-body {
    display: flex;
    flex-direction: column;
    gap: 1.5rem;
  }

  .settings-section {
    display: flex;
    flex-direction: column;
    gap: 0.65rem;
  }

  .section-title-bar {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .settings-subtitle {
    font-size: 0.95rem;
    font-weight: 700;
    color: #f0f6fc;
    margin: 0;
  }

  .settings-desc {
    font-size: 0.84rem;
    color: #94a3b8;
    margin: 0;
    line-height: 1.45;
  }

  .settings-desc code {
    background: rgba(99, 102, 241, 0.15);
    color: #c7d2fe;
    padding: 1px 5px;
    border-radius: 4px;
    font-size: 0.85em;
  }

  .badge-count {
    font-size: 0.76rem;
    padding: 2px 8px;
    border-radius: 9999px;
    background: rgba(255, 255, 255, 0.08);
    color: #94a3b8;
    border: 1px solid rgba(255, 255, 255, 0.12);
  }

  .badge-count-active {
    background: rgba(34, 197, 94, 0.15);
    color: #86efac;
    border-color: rgba(34, 197, 94, 0.35);
  }

  .alert-success {
    background: rgba(34, 197, 94, 0.12);
    border: 1px solid rgba(34, 197, 94, 0.35);
    color: #86efac;
  }

  .alert-danger {
    background: rgba(239, 68, 68, 0.12);
    border: 1px solid rgba(239, 68, 68, 0.35);
    color: #fca5a5;
  }

  .empty-models-box {
    background: #0d1117;
    border: 1px dashed #30363d;
    border-radius: 8px;
    padding: 1.25rem;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    font-size: 0.88rem;
  }

  .server-active-box {
    border: 1px solid rgba(34, 197, 94, 0.4);
    background: rgba(34, 197, 94, 0.06);
    flex-direction: row;
    align-items: flex-start;
    gap: 1rem;
  }

  .server-active-icon {
    font-size: 1.4rem;
    line-height: 1;
    margin-top: 2px;
  }

  .server-active-details {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .server-active-title {
    color: #86efac;
    margin: 0;
    font-size: 0.95rem;
  }

  .alert-info-server {
    background: rgba(14, 165, 233, 0.12);
    border: 1px solid rgba(14, 165, 233, 0.35);
    color: #bae6fd;
    margin-bottom: 1rem;
  }

  .empty-models-box p {
    margin: 0;
  }

  .hint-muted {
    color: #8b949e;
    font-size: 0.82rem;
    line-height: 1.4;
  }

  .recommended-downloads {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    margin-top: 0.25rem;
  }

  .model-download-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.7rem 0.9rem;
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 8px;
    text-decoration: none;
    transition: background 0.15s, border-color 0.15s;
  }

  .model-download-card:hover {
    background: rgba(99, 102, 241, 0.1);
    border-color: rgba(99, 102, 241, 0.3);
  }

  .dl-info {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }

  .dl-title {
    font-size: 0.85rem;
    font-weight: 600;
    color: #f1f5f9;
  }

  .dl-desc {
    font-size: 0.78rem;
    color: #94a3b8;
  }

  .dl-action {
    font-size: 0.8rem;
    font-weight: 600;
    color: #86efac;
    padding: 4px 8px;
    border-radius: 6px;
    background: rgba(34, 197, 94, 0.12);
  }

  .models-grid {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    max-height: 280px;
    overflow-y: auto;
  }

  .model-card {
    background: #0d1117;
    border: 1px solid #30363d;
    border-radius: 8px;
    padding: 0.75rem 1rem;
    transition: border-color 0.15s, background 0.15s;
  }

  .model-card:hover {
    border-color: #484f58;
  }

  .model-card-active {
    border-color: rgba(34, 197, 94, 0.6);
    background: rgba(34, 197, 94, 0.04);
  }

  .model-card-heavy {
    border-left: 3px solid #eab308;
  }

  .model-card-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 1rem;
  }

  .model-name-col {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    flex: 1;
    min-width: 0;
  }

  .model-title {
    font-size: 0.9rem;
    font-weight: 700;
    color: #f0f6fc;
    word-break: break-all;
  }

  .model-tags {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    flex-wrap: wrap;
  }

  .tag {
    font-size: 0.72rem;
    font-weight: 600;
    padding: 2px 6px;
    border-radius: 4px;
  }

  .tag-size {
    background: rgba(255, 255, 255, 0.08);
    color: #cbd5e1;
  }

  .tag-arch {
    background: rgba(99, 102, 241, 0.15);
    color: #c7d2fe;
  }

  .tag-loaded {
    background: rgba(34, 197, 94, 0.18);
    color: #86efac;
    border: 1px solid rgba(34, 197, 94, 0.4);
  }

  .tag-warning {
    background: rgba(234, 179, 8, 0.15);
    color: #fde68a;
  }

  .tag-ok {
    background: rgba(34, 197, 94, 0.1);
    color: #86efac;
  }

  .tag-ctx {
    background: rgba(168, 85, 247, 0.15);
    color: #d8b4fe;
    border: 1px solid rgba(168, 85, 247, 0.3);
  }

  .tag-rec {
    background: rgba(56, 189, 248, 0.15);
    color: #7dd3fc;
    border: 1px solid rgba(56, 189, 248, 0.3);
  }

  .model-path-hint {
    font-size: 0.7rem;
    color: #64748b;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .model-path-hint code {
    background: none;
    color: inherit;
    padding: 0;
  }

  .model-card-actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-shrink: 0;
  }

  .btn-sm {
    padding: 0.45rem 0.85rem;
    border-radius: 6px;
    font-size: 0.8rem;
    font-weight: 600;
    cursor: pointer;
    font-family: inherit;
    border: none;
    transition: all 0.15s ease;
  }

  .btn-xs {
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.15);
    color: #c7d2fe;
    padding: 3px 8px;
    border-radius: 5px;
    font-size: 0.74rem;
    cursor: pointer;
    font-family: inherit;
  }

  .btn-xs:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.14);
  }

  .btn-xs:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .advice-box {
    background: #0d1117;
    border: 1px solid #30363d;
    border-radius: 8px;
    padding: 0.85rem 1rem;
    font-size: 0.83rem;
    color: #94a3b8;
    line-height: 1.5;
  }

  .advice-box strong {
    color: #f0f6fc;
    display: block;
    margin-bottom: 0.4rem;
  }

  .advice-box ul {
    margin: 0;
    padding-left: 1.2rem;
  }

  .advice-box li {
    margin-bottom: 0.3rem;
  }

  .advice-box code {
    background: rgba(99, 102, 241, 0.15);
    color: #c7d2fe;
    padding: 1px 4px;
    border-radius: 3px;
  }

  /* ─── Carte Proposition Paramètres Recommandés GGUF ─── */
  .recommendation-card {
    background: linear-gradient(135deg, rgba(99, 102, 241, 0.12) 0%, rgba(56, 189, 248, 0.08) 100%);
    border: 1px solid rgba(99, 102, 241, 0.45);
    border-radius: 8px;
    padding: 0.95rem 1.1rem;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .rec-header {
    display: flex;
    align-items: flex-start;
    gap: 0.65rem;
  }

  .rec-icon {
    font-size: 1.35rem;
    line-height: 1;
    margin-top: 2px;
  }

  .rec-header-text {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    flex: 1;
  }

  .rec-title {
    font-size: 0.92rem;
    font-weight: 700;
    color: #f0f6fc;
    margin: 0;
  }

  .rec-subtitle {
    font-size: 0.79rem;
    color: #94a3b8;
    margin: 0;
    line-height: 1.45;
  }

  .rec-subtitle code {
    background: rgba(99, 102, 241, 0.2);
    color: #c7d2fe;
    padding: 1px 4px;
    border-radius: 3px;
  }

  .rec-comparison-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(210px, 1fr));
    gap: 0.55rem;
  }

  .rec-comparison-item {
    background: rgba(13, 17, 23, 0.65);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 6px;
    padding: 0.45rem 0.65rem;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .rec-comp-label {
    font-size: 0.7rem;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: #a5b4fc;
  }

  .rec-comp-values {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    font-size: 0.79rem;
  }

  .rec-val-current {
    color: #94a3b8;
  }

  .rec-arrow {
    color: #38bdf8;
    font-weight: 700;
  }

  .rec-val-recommended {
    color: #86efac;
    font-weight: 600;
  }

  .rec-actions {
    display: flex;
    align-items: center;
    gap: 0.65rem;
    flex-wrap: wrap;
    margin-top: 0.25rem;
  }

  .rec-btn-apply {
    background: #238636;
    color: #ffffff;
  }

  .rec-btn-apply:hover {
    background: #2ea043;
  }

  .rec-btn-dismiss {
    background: rgba(255, 255, 255, 0.06);
    color: #cbd5e1;
  }

  .rec-btn-dismiss:hover {
    background: rgba(255, 255, 255, 0.12);
    color: #f0f6fc;
  }

  .checkbox-label {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    cursor: pointer;
    font-size: 0.82rem;
    font-weight: 600;
    color: #f0f6fc;
    user-select: none;
  }

  .checkbox-label input[type="checkbox"] {
    accent-color: #238636;
    width: 16px;
    height: 16px;
    cursor: pointer;
  }

  /* ─── Styles Paramètres Inférence & Accélération ─── */
  .save-toast {
    font-size: 0.76rem;
    font-weight: 600;
    color: #86efac;
    background: rgba(34, 197, 94, 0.15);
    border: 1px solid rgba(34, 197, 94, 0.35);
    padding: 3px 8px;
    border-radius: 6px;
  }

  .settings-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: 0.85rem;
  }

  .setting-card {
    background: #0d1117;
    border: 1px solid #30363d;
    border-radius: 8px;
    padding: 0.9rem 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    transition: border-color 0.15s ease;
  }

  .setting-card:hover {
    border-color: #484f58;
  }

  .setting-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.5rem;
  }

  .setting-title-group {
    display: flex;
    align-items: center;
    gap: 0.45rem;
  }

  .setting-icon {
    font-size: 1.1rem;
  }

  .setting-label {
    font-size: 0.86rem;
    font-weight: 600;
    color: #f0f6fc;
    cursor: pointer;
  }

  .setting-desc {
    font-size: 0.8rem;
    color: #8b949e;
    margin: 0;
    line-height: 1.4;
  }

  .text-success {
    color: #86efac;
  }

  .text-warning {
    color: #facc15;
  }

  /* Toggle Switch */
  .toggle-switch {
    position: relative;
    display: inline-block;
    width: 44px;
    height: 24px;
    flex-shrink: 0;
    cursor: pointer;
  }

  .toggle-switch input {
    opacity: 0;
    width: 0;
    height: 0;
  }

  .toggle-slider {
    position: absolute;
    cursor: pointer;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background-color: #30363d;
    transition: 0.2s;
    border-radius: 24px;
    border: 1px solid #484f58;
  }

  .toggle-slider:before {
    position: absolute;
    content: "";
    height: 16px;
    width: 16px;
    left: 3px;
    bottom: 3px;
    background-color: #c9d1d9;
    transition: 0.2s;
    border-radius: 50%;
  }

  .toggle-switch input:checked + .toggle-slider {
    background-color: #238636;
    border-color: #2ea043;
  }

  .toggle-switch input:checked + .toggle-slider:before {
    transform: translateX(20px);
    background-color: #ffffff;
  }

  /* Input with unit */
  .input-with-unit {
    display: inline-flex;
    align-items: center;
    background: #161b22;
    border: 1px solid #30363d;
    border-radius: 6px;
    padding: 0 0.5rem;
    gap: 0.25rem;
  }

  .input-with-unit:focus-within {
    border-color: #58a6ff;
  }

  .input-number {
    background: transparent;
    border: none;
    color: #f0f6fc;
    font-family: inherit;
    font-size: 0.86rem;
    font-weight: 600;
    width: 52px;
    padding: 0.35rem 0;
    text-align: right;
    outline: none;
  }

  .unit-label {
    font-size: 0.78rem;
    color: #8b949e;
    user-select: none;
  }

  /* Accordion Paramètres Avancés */
  .advanced-accordion {
    background: #0d1117;
    border: 1px solid #30363d;
    border-radius: 8px;
    overflow: hidden;
  }

  .advanced-summary {
    padding: 0.75rem 1rem;
    font-size: 0.88rem;
    font-weight: 600;
    color: #c9d1d9;
    cursor: pointer;
    user-select: none;
    background: rgba(255, 255, 255, 0.02);
    transition: background 0.15s ease, color 0.15s ease;
  }

  .advanced-summary:hover {
    background: rgba(255, 255, 255, 0.05);
    color: #f0f6fc;
  }

  .advanced-content {
    padding: 1rem;
    border-top: 1px solid #21262d;
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  .advanced-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: 0.85rem;
  }

  .advanced-field {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .advanced-field-full {
    grid-column: 1 / -1;
  }

  .adv-label {
    font-size: 0.8rem;
    font-weight: 600;
    color: #c9d1d9;
  }

  .adv-input, .adv-select {
    background: #161b22;
    border: 1px solid #30363d;
    color: #f0f6fc;
    font-family: inherit;
    font-size: 0.82rem;
    padding: 0.45rem 0.65rem;
    border-radius: 6px;
    outline: none;
    transition: border-color 0.15s ease;
  }

  .adv-input:focus, .adv-select:focus {
    border-color: #58a6ff;
  }

  .adv-hint {
    font-size: 0.74rem;
    color: #8b949e;
    line-height: 1.35;
  }

  .advanced-actions {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-top: 0.25rem;
    flex-wrap: wrap;
  }

  .btn-secondary {
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.15);
    color: #c9d1d9;
  }

  .btn-secondary:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.12);
    color: #f0f6fc;
  }

  /* ─── Bloc Serveur d'Inférence Personnalisé ─── */
  .custom-server-block {
    background: rgba(99, 102, 241, 0.05);
    border: 1px solid rgba(99, 102, 241, 0.25);
    border-radius: 8px;
    padding: 0.85rem 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.65rem;
    margin-top: 0.5rem;
  }

  .custom-server-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.5rem;
  }

  .custom-server-title {
    font-size: 0.88rem;
    font-weight: 700;
    color: #e2e8f0;
  }

  .custom-server-inputs-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: 0.75rem;
  }

  .key-label-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .key-toggle-btn {
    background: none;
    border: none;
    color: #818cf8;
    font-size: 0.72rem;
    cursor: pointer;
    padding: 0;
  }

  .key-toggle-btn:hover {
    text-decoration: underline;
  }

  .custom-server-fetch-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    flex-wrap: wrap;
    margin-top: 0.25rem;
  }

  .fetch-notice {
    font-size: 0.78rem;
    line-height: 1.3;
  }

  .fetch-success {
    color: #86efac;
  }

  .fetch-error {
    color: #f87171;
  }
</style>
