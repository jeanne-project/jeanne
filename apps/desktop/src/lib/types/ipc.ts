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
  max_recommended_context?: number;
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

export interface ModelRecommendedParams {
  context_size?: number | null;
  temperature?: number | null;
  top_p?: number | null;
  top_k?: number | null;
}

export interface DiscoveredModel {
  name: string;
  path: string;
  size_bytes: number;
  size_formatted: string;
  architecture?: string;
  is_loaded: boolean;
  fits_ram: boolean;
  context_length?: number | null;
  recommended_params?: ModelRecommendedParams | null;
}

export interface LocalEngineConfig {
  model_path?: string | null;
  context_size: number;
  threads?: number | null;
  use_vulkan: boolean;
  use_gpu: boolean;
  gpu_layers?: number | null;
  generation_timeout_secs: number;
  temperature: number;
  top_p?: number | null;
  top_k?: number | null;
  max_tokens: number;
  allow_extended_context?: boolean;
  daemon_endpoint?: string | null;
  daemon_api_key?: string | null;
  daemon_model?: string | null;
  expected_sha256?: string | null;
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
  get_local_engine_config(): Promise<LocalEngineConfig>;
  update_local_engine_config(config: LocalEngineConfig): Promise<LocalEngineConfig>;
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
  toggle_voice_pipeline(active: boolean): Promise<boolean>;
  get_voice_status(): Promise<VoiceStatus>;
  get_voice_config(): Promise<VoiceConfig>;
  update_voice_config(config: VoiceConfig): Promise<VoiceConfig>;
  list_audio_devices(): Promise<AudioDevicesReport>;
  transcribe_pcm_chunk(samples: number[], sample_rate: number): Promise<string>;
  synthesize_text_to_audio(text: string): Promise<number[]>;
  fetch_remote_server_models(endpoint: string, api_key?: string | null): Promise<string[]>;
  is_inference_ready(): Promise<boolean>;
}

export interface VoiceConfig {
  whisper_model_path?: string | null;
  piper_model_path?: string | null;
  selected_input_device?: string | null;
  selected_output_device?: string | null;
  remote_stt_endpoint?: string | null;
}

export type VoiceState = 'Idle' | 'Listening' | 'Transcribing' | 'Thinking' | 'Speaking' | 'Error';

export interface AudioDevice {
  id: string;
  name: string;
  default_sample_rate: number;
  is_default: boolean;
  is_input: boolean;
}

export interface AudioDevicesReport {
  input_devices: AudioDevice[];
  output_devices: AudioDevice[];
  default_input_name: string | null;
  default_output_name: string | null;
}

export interface VoiceStatus {
  is_active: boolean;
  state: VoiceState;
  input_sample_rate: number;
  memory_allocated_mb: number;
  last_transcription_latency_ms: number;
  last_synthesis_ttfb_ms: number;
  active_device_name: string | null;
}

