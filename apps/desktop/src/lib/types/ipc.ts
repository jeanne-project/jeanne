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

export interface IpcCommands {
  search_notes(query: string, limit?: number): Promise<SearchResult[]>;
  capture_quick_note(content: string): Promise<string>;
  open_note_in_editor(file_path: string): Promise<void>;
  get_vault_stats(): Promise<VaultStats>;
  exit_app(): Promise<void>;
}
