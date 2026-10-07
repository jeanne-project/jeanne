// Plugin Jeanne `pdf-parser` : JSON-RPC 2.0 sur stdio (une requête NDJSON par ligne).
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"time"
)

type request struct {
	JSONRPC string          `json:"jsonrpc"`
	Method  string          `json:"method"`
	Params  json.RawMessage `json:"params"`
	ID      uint64          `json:"id"`
}

type response struct {
	JSONRPC string    `json:"jsonrpc"`
	Result  any       `json:"result,omitempty"`
	Error   *rpcError `json:"error,omitempty"`
	ID      uint64    `json:"id"`
}

type parseParams struct {
	FilePath string       `json:"file_path"`
	Options  parseOptions `json:"options"`
}

// internalTimeout est inférieur au watchdog Rust (120 s) pour renvoyer -32003 proprement.
const internalTimeout = 110 * time.Second

func handle(req request) response {
	resp := response{JSONRPC: "2.0", ID: req.ID}
	if req.Method != "parse_document" {
		resp.Error = &rpcError{codeMethodUnknown, "méthode inconnue: " + req.Method}
		return resp
	}
	var p parseParams
	if err := json.Unmarshal(req.Params, &p); err != nil {
		resp.Error = &rpcError{codeInvalidParams, "params invalides: " + err.Error()}
		return resp
	}
	type out struct {
		res *parseResult
		err error
	}
	ch := make(chan out, 1)
	go func() {
		r, err := parsePDF(p.FilePath, p.Options)
		ch <- out{r, err}
	}()
	select {
	case o := <-ch:
		if o.err != nil {
			if re, ok := o.err.(*rpcError); ok {
				resp.Error = re
			} else {
				resp.Error = &rpcError{codeCorrupted, o.err.Error()}
			}
			return resp
		}
		resp.Result = o.res
	case <-time.After(internalTimeout):
		resp.Error = &rpcError{codeTimeout, "délai de traitement dépassé"}
	}
	return resp
}

func run(in io.Reader, out io.Writer) {
	reader := bufio.NewReaderSize(in, 64*1024)
	enc := json.NewEncoder(out)
	for {
		raw, err := reader.ReadBytes('\n')
		if len(raw) > 0 && len(trimSpace(raw)) > 0 {
			var req request
			if jerr := json.Unmarshal(raw, &req); jerr != nil {
				_ = enc.Encode(response{JSONRPC: "2.0", Error: &rpcError{codeParseError, "JSON invalide: " + jerr.Error()}})
			} else {
				_ = enc.Encode(handle(req))
			}
		}
		if err != nil {
			return
		}
	}
}

func trimSpace(b []byte) []byte {
	for len(b) > 0 && (b[0] == ' ' || b[0] == '\n' || b[0] == '\r' || b[0] == '\t') {
		b = b[1:]
	}
	for len(b) > 0 && (b[len(b)-1] == ' ' || b[len(b)-1] == '\n' || b[len(b)-1] == '\r' || b[len(b)-1] == '\t') {
		b = b[:len(b)-1]
	}
	return b
}

func main() {
	fmt.Fprintln(os.Stderr, "pdf-parser: démarrage")
	run(os.Stdin, os.Stdout)
}
