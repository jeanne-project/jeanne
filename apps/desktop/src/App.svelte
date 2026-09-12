<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  let coreVersion = 'Chargement...';

  onMount(async () => {
    try {
      coreVersion = await invoke('get_core_version');
    } catch (err) {
      coreVersion = 'Erreur liaison core';
    }
  });
</script>

<main style="font-family: system-ui, sans-serif; padding: 2rem; max-width: 800px; margin: auto;">
  <h1>Jeanne</h1>
  <p style="color: #666; font-style: italic;">Elle est enfin là. Votre second cerveau local et assistant de réunion.</p>
  <hr style="margin: 1.5rem 0; border: 0; border-top: 1px solid #ddd;" />
  <p><strong>Moteur applicatif (jeanne-core) :</strong> v{coreVersion}</p>
  <p><strong>Raccourci d'accès rapide :</strong> <code>Alt + Espace</code></p>
</main>
