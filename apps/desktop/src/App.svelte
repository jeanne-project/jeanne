<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  let coreVersion = $state('Loading...');

  onMount(async () => {
    try {
      coreVersion = await invoke('get_core_version');
    } catch {
      coreVersion = 'Error connecting to jeanne-core';
    }
  });
</script>

<main style="font-family: system-ui, -apple-system, sans-serif; padding: 2rem; max-width: 800px; margin: auto;">
  <h1>Jeanne</h1>
  <p style="color: #666; font-style: italic;">Elle est enfin là. Local-first AI knowledge assistant and meeting co-pilot.</p>
  <hr style="margin: 1.5rem 0; border: 0; border-top: 1px solid #ddd;" />
  <p><strong>Core Engine:</strong> v{coreVersion} (Rust Edition 2024)</p>
  <p><strong>Frontend:</strong> Svelte 5 (Runes) + Tauri v2</p>
  <p><strong>Quick-Access Overlay:</strong> <code>Alt + Space</code></p>
</main>
