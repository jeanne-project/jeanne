export interface SearchResult {
  chunk_id: string;
  file_path: string;
  title: string;
  snippet: string;
  score: number;
  statut: 'actif' | 'obsolete' | 'archive';
  date_creation: string;
}

export interface VaultStats {
  total_files: number;
  total_chunks: number;
  last_scan_timestamp: number;
}

export interface HardwareInfo {
  total_system_ram_mb: number;
  available_ram_mb: number;
  vulkan_device_name: string | null;
  vulkan_supported: boolean;
  recommended_model_loaded: boolean;
}

export interface LocalInferenceStats {
  prompt_tokens: number;
  generated_tokens: number;
  tokens_per_second: number;
  memory_allocated_mb: number;
}

export interface TaskItem {
  file_path: string;
  line_number: number;
  content: string;
  checked: boolean;
  created_at?: string;
}

export interface SnippetItem {
  key: string;
  title: string;
  content: string;
}

export interface DiscoveredModel {
  name: string;
  path: string;
  size_bytes: number;
  size_formatted: string;
  architecture?: string;
  is_loaded: boolean;
  fits_ram: boolean;
}

export interface IpcCommands {
  search_notes(query: string, limit?: number): Promise<SearchResult[]>;
  capture_quick_note(content: string): Promise<string>;
  open_note_in_editor(file_path: string): Promise<void>;
  get_vault_stats(): Promise<VaultStats>;
  exit_app(): Promise<void>;
  load_local_model(model_path?: string): Promise<void>;
  unload_local_model(): Promise<void>;
  get_hardware_profile(): Promise<HardwareInfo>;
  get_local_inference_stats(): Promise<LocalInferenceStats>;
  get_default_model_path(): Promise<string>;
  get_models_directory(): Promise<string>;
  list_available_models(): Promise<DiscoveredModel[]>;
  execute_todo(content: string): Promise<string>;
  get_vault_tasks(limit?: number): Promise<TaskItem[]>;
  toggle_vault_task(file_path: string, line_number: number, checked: boolean): Promise<void>;
  execute_log(content: string): Promise<string>;
  execute_meeting(title: string): Promise<string>;
  execute_bookmark(url: string, comment?: string): Promise<string>;
  get_snippets(): Promise<SnippetItem[]>;
  evaluate_math(expression: string): Promise<number>;
  ai_process_clipboard(action: string, text: string, param?: string): Promise<string>;
  ask_vault(question: string): Promise<string>;
}
