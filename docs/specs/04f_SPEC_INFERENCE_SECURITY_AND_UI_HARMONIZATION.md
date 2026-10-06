# Spécification Technique : Sécurité, Confidentialité PII & Harmonisation UI du Serveur Distant (04f_SPEC_INFERENCE_SECURITY_AND_UI_HARMONIZATION.md)

Ce document formalise les exigences techniques et d'architecture pour la résolution des incohérences de Priorité 1 (INC-01, INC-02, INC-03) identifiées lors de l'audit architectural.

---

## 1. Contexte & Objectifs

Suite à l'implémentation de la spécification `04d` permettant de connecter un serveur d'inférence personnalisé (Ollama, LM Studio, vLLM, API cloud OpenAI), un audit de la base de code a mis en lumière trois anomalies critiques liées à cette fonctionnalité :
1. **INC-01 (Confidentialité / PII)** : Absence de masquage PII lors de l'envoi de requêtes vers des serveurs d'inférence distants / externes.
2. **INC-02 (Sécurité / Persistance)** : Stockage en clair de la clé d'API (`daemon_api_key`) dans `.jeanne/local_llm_settings.json` au lieu d'utiliser le gestionnaire de trousseau d'accès OS natif (`KeyringManager`).
3. **INC-03 (Ergonomie & Clarté UI)** : Affichage d'alertes d'erreur alarmistes (`⚠️ Aucun modèle .gguf détecté`) et modales d'aide inadaptées dans l'interface lorsque le serveur d'inférence est actif sans fichier GGUF local.

---

## 2. Spécification Détaillée par Composant

### 2.1 INC-01 : Masquage & Démasquage PII sur Serveurs Distants (`crates/core`)

#### Exigences :
1. **Détection d'Endpoint Distant** :
   - Une fonction `is_remote_endpoint(url: &str) -> bool` détermine si l'URL cible pointe vers un serveur externe ou distant.
   - Les adresses `127.0.0.1`, `localhost`, `0.0.0.0`, `::1` et les ports locaux sont considérés comme locaux (pas de transit sur Internet).
   - Tout endpoint distant (ex. `https://api.openai.com/v1`, IP publique, domaine externe) est qualifié de distant.
2. **Masquage Préventif Outgoing** :
   - Si `is_remote_endpoint` est vrai, le prompt transitant par `try_stream_from_local_daemon()` est obligatoirement filtré par `PiiSession::new().mask_text(&prompt)` avant la construction du payload JSON HTTP.
   - Les emails (`[EMAIL_N]`), numéros de téléphone (`[PHONE_N]`) et coordonnées financières (`[FINANCIAL_N]`) sont masqués.
3. **Démasquage au Vol Incoming (Streaming SSE)** :
   - Les tokens reçus par la connexion SSE sont passés à un `PiiSlidingBuffer` initialisé avec la table inverse `session.reverse_map`.
   - Les tokens démasqués sont transmis au canal mpsc utilisateur.
   - Lors de la complétion du flux (`[DONE]`, annulation ou timeout), le buffer résiduel est vidé via `buffer.flush()`.
4. **Préservation des performances locales** :
   - Pour les serveurs locaux (`127.0.0.1:11434`, `127.0.0.1:8080`, etc.), aucun surcoût de regex n'est imposé, le flux passe directement.

---

### 2.2 INC-02 : Stockage Sécurisé de la Clé d'API via `KeyringManager` (`crates/core` & `src-tauri`)

#### Exigences :
1. **Hygiène du Fichier de Paramètres** :
   - Le fichier `.jeanne/local_llm_settings.json` dans le coffre ne doit JAMAIS contenir la valeur secrète de la clé d'API en texte clair.
   - Le champ sérialisé sur disque doit être omis (`#[serde(skip_serializing)]` ou sanitizé à `None` avant écriture sur disque).
2. **Utilisation du Trousseau Système (`KeyringManager`)** :
   - Service : `"jeanne"`, Clé d'accès : `"daemon_api_key"`.
   - Lors de l'appel IPC `update_local_engine_config` :
     - Si une clé non vide est fournie, `KeyringManager::set_api_key("jeanne", "daemon_api_key", &key)` est invoqué.
     - Si la clé est explicitement effacée (vide ou `None`), `KeyringManager::delete_api_key("jeanne", "daemon_api_key")` est exécuté.
   - Au démarrage de l'application dans `setup()` :
     - La clé est récupérée via `KeyringManager::get_api_key("jeanne", "daemon_api_key")`.
     - Si le keyring de l'OS est indisponible ou retourne une erreur (ex. conteneur CI sans session D-Bus/SecretService), un repli mémoire propre (graceful fallback) est appliqué sans faire planter l'application.

---

### 2.3 INC-03 : Ergonomie & Cohérence dans l'Interface Utilisateur (`App.svelte`)

#### Exigences :
1. **Section 2 des Paramètres (Modèles Détectés)** :
   - Remplacer la boîte d'alerte jaune `⚠️ Aucun modèle .gguf détecté` par un état informatif positif lorsque `isCustomServerConfigured` est vrai :
     ```svelte
     {#if availableModels.length === 0}
       {#if isCustomServerConfigured}
         <div class="server-active-box">
           <p><strong>🟢 Inférence assurée par le serveur personnalisé</strong></p>
           <p class="hint-muted">
             Votre serveur ({engineConfig.daemon_endpoint}) avec le modèle <strong>{engineConfig.daemon_model || 'par défaut'}</strong> est actif. L'installation de fichiers <code>.gguf</code> locaux est entièrement facultative.
           </p>
         </div>
       {:else}
         <!-- Boîte d'alerte et liens de téléchargement standard -->
       {/if}
     {/if}
     ```
2. **Tableau de Bord & Guide d'Installation** :
   - Dans le guide d'installation (`showModelSetupModal`), ajouter une bannière d'information claire si `isCustomServerConfigured` est actif :
     « *Note : Vous disposez déjà d'un serveur d'inférence actif ({engineConfig.daemon_endpoint}). Ce guide est uniquement utile si vous désirez fonctionner 100% hors-ligne sans serveur.* »
   - Ajuster l'action du bouton `Charger un Modèle Local (Optionnel)` pour éviter une notification d'erreur technique brutale si aucun modèle n'est sur disque.

---

## 3. Matrice de Tests & Critères d'Acceptation

| ID Test | Composant | Description | Résultat Attendu |
| :--- | :--- | :--- | :--- |
| **TEST-04F-01** | `crates/core` | Détection d'URL distante vs locale | `is_remote_endpoint("https://api.openai.com/v1") == true`, `is_remote_endpoint("http://127.0.0.1:11434") == false` |
| **TEST-04F-02** | `crates/core` | Masquage PII streaming sur endpoint distant | Prompt avec email et IBAN anonymisé à l'envoi HTTP, démasqué au vol sur le flux SSE |
| **TEST-04F-03** | `crates/core` | Pas de masquage PII sur endpoint local (zéro surcoût) | Les tokens et prompts sur `127.0.0.1` transitent sans altération |
| **TEST-04F-04** | `crates/core` | Intégration `KeyringManager` pour la clé d'API | Clé persistée dans le trousseau, absente du JSON `.jeanne/local_llm_settings.json` |
| **TEST-04F-05** | `desktop` UI | Affichage conditionnel dans les paramètres | Pas de warning jaune alarmiste lorsque le serveur d'inférence est actif |
