# 03 - Plugin Protocol & Extensibility Specifications

## 1. Architectural Principles
1. **Subprocess Isolation**: Plugins are standalone executables (compiled in Go, Rust, C++, etc.). A crash or panic in a plugin must never bring down the main Jeanne desktop process.
2. **Standardized Protocol**: Communication operates strictly via **JSON-RPC 2.0** over standard I/O (`stdin` for requests, `stdout` for responses, `stderr` for logs).
3. **Ephemeral Lifecycle (`on_demand`)**: To maintain a lean memory profile on 16 GB machines, document parsing plugins are spawned on demand, process the target file, stream results, and immediately exit to free memory.

## 2. Plugin Manifest (`plugin.json`)
Every plugin resides in its own folder (`~/.config/secondbrain/plugins/<id>/` or `%APPDATA%/SecondBrain/plugins/<id>/`) with a root `plugin.json`:

```json
{
  "schema_version": "1.0",
  "id": "org.jeanneproject.parser.pdf",
  "name": "PDF Structured Parser",
  "version": "1.0.0",
  "entrypoint": {
    "windows": "bin/pdf-parser.exe",
    "linux": "bin/pdf-parser"
  },
  "lifecycle": "on_demand",
  "timeout_seconds": 120,
  "capabilities": [
    {
      "type": "document_parser",
      "supported_extensions": [".pdf"],
      "supported_mimetypes": ["application/pdf"]
    }
  ]
}
```

## 3. JSON-RPC 2.0 Interface (`parse_document`)

### Request dispatched by Jeanne to Plugin (`stdin`):
```json
{
  "jsonrpc": "2.0",
  "method": "parse_document",
  "params": {
    "file_path": "/absolute/path/to/document.pdf",
    "options": {
      "extract_images": false
    }
  },
  "id": 1
}
```

### Expected Response from Plugin (`stdout`):
```json
{
  "jsonrpc": "2.0",
  "result": {
    "title": "Extracted Document Title",
    "content_markdown": "# Structured Markdown content extracted from document...",
    "metadata": {
      "author": "Author Name",
      "date": "2026-09-12",
      "page_count": 8
    },
    "attachments": []
  },
  "id": 1
}
```

### Standardized Error Codes
* `-32700`: Parse error (Invalid JSON payload).
* `-32601`: Method not found.
* `-32001`: File not found or unreadable.
* `-32002`: Corrupted file or unsupported sub-format.
* `-32003`: Internal processing timeout.
