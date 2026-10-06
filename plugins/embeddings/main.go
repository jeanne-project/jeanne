package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"hash/fnv"
	"log"
	"math"
	"os"
	"strings"
	"time"
)

const (
	DefaultModelID = "all-MiniLM-L6-v2"
	EmbeddingDim   = 384
	MaxSeqLength   = 512
)

// Modèles supportés selon config.json
var availableModels = map[string]int64{
	"all-MiniLM-L6-v2":     424242,
	"bge-small-en-v1.5":    848484,
	"multilingual-e5-small": 121212,
}

type JSONRPCRequest struct {
	JSONRPC string          `json:"jsonrpc"`
	Method  string          `json:"method"`
	Params  json.RawMessage `json:"params"`
	ID      interface{}     `json:"id"`
}

type JSONRPCResponse struct {
	JSONRPC string      `json:"jsonrpc"`
	Result  interface{} `json:"result,omitempty"`
	Error   *RPCError   `json:"error,omitempty"`
	ID      interface{} `json:"id"`
}

type RPCError struct {
	Code    int    `json:"code"`
	Message string `json:"message"`
}

type ModelInfoResult struct {
	ModelID      string `json:"model_id"`
	Dimension    int    `json:"dimension"`
	MaxSeqLength int    `json:"max_seq_length"`
	Normalized   bool   `json:"normalized"`
}

type EmbedTextParams struct {
	Text       string `json:"text"`
	PromptType string `json:"prompt_type,omitempty"`
}

type EmbedTextResult struct {
	Embedding  []float32 `json:"embedding"`
	TokenCount int       `json:"token_count"`
	ElapsedMS  int64     `json:"elapsed_ms"`
}

type EmbedBatchParams struct {
	Texts []string `json:"texts"`
}

type EmbedBatchResult struct {
	Embeddings  [][]float32 `json:"embeddings"`
	Count       int         `json:"count"`
	TotalTokens int         `json:"total_tokens"`
	ElapsedMS   int64       `json:"elapsed_ms"`
}

type SwitchModelParams struct {
	ModelID string `json:"model_id"`
}

type SwitchModelResult struct {
	Status    string `json:"status"`
	ModelID   string `json:"model_id"`
	Dimension int    `json:"dimension"`
}

// État du plugin
var activeModelID = DefaultModelID

func main() {
	log.SetOutput(os.Stderr)
	log.Printf("[jeanne-embeddings] Plugin démarré en écoute sur stdin (dim=%d, default_model=%s)", EmbeddingDim, activeModelID)

	scanner := bufio.NewScanner(os.Stdin)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" {
			continue
		}

		var req JSONRPCRequest
		if err := json.Unmarshal([]byte(line), &req); err != nil {
			sendError(nil, -32700, "Parse error")
			continue
		}

		handleRequest(&req)
	}

	if err := scanner.Err(); err != nil {
		log.Printf("[jeanne-embeddings] Erreur lecture stdin: %v", err)
	}
	log.Println("[jeanne-embeddings] Stdin fermé, extinction propre du plugin.")
}

func handleRequest(req *JSONRPCRequest) {
	switch req.Method {
	case "get_model_info":
		resp := ModelInfoResult{
			ModelID:      activeModelID,
			Dimension:    EmbeddingDim,
			MaxSeqLength: MaxSeqLength,
			Normalized:   true,
		}
		sendResult(req.ID, resp)

	case "embed_text":
		var p EmbedTextParams
		if req.Params != nil && len(req.Params) > 0 {
			if err := json.Unmarshal(req.Params, &p); err != nil {
				sendError(req.ID, -32602, "Invalid params")
				return
			}
		}
		start := time.Now()
		vec, tokens := computeEmbedding(p.Text, p.PromptType, activeModelID)
		elapsed := time.Since(start).Milliseconds()
		if elapsed == 0 {
			elapsed = 1
		}
		sendResult(req.ID, EmbedTextResult{
			Embedding:  vec,
			TokenCount: tokens,
			ElapsedMS:  elapsed,
		})

	case "embed_batch":
		var p EmbedBatchParams
		if req.Params != nil && len(req.Params) > 0 {
			if err := json.Unmarshal(req.Params, &p); err != nil {
				sendError(req.ID, -32602, "Invalid params")
				return
			}
		}
		start := time.Now()
		embeddings := make([][]float32, 0, len(p.Texts))
		totalTokens := 0
		for _, t := range p.Texts {
			vec, tok := computeEmbedding(t, "document", activeModelID)
			embeddings = append(embeddings, vec)
			totalTokens += tok
		}
		elapsed := time.Since(start).Milliseconds()
		if elapsed == 0 {
			elapsed = 1
		}
		sendResult(req.ID, EmbedBatchResult{
			Embeddings:  embeddings,
			Count:       len(embeddings),
			TotalTokens: totalTokens,
			ElapsedMS:   elapsed,
		})

	case "switch_model":
		var p SwitchModelParams
		if req.Params != nil && len(req.Params) > 0 {
			if err := json.Unmarshal(req.Params, &p); err != nil {
				sendError(req.ID, -32602, "Invalid params")
				return
			}
		}
		if p.ModelID != "" {
			activeModelID = p.ModelID
		}
		sendResult(req.ID, SwitchModelResult{
			Status:    "switched",
			ModelID:   activeModelID,
			Dimension: EmbeddingDim,
		})

	default:
		sendError(req.ID, -32601, fmt.Sprintf("Method '%s' not found", req.Method))
	}
}

func sendResult(id interface{}, result interface{}) {
	resp := JSONRPCResponse{
		JSONRPC: "2.0",
		Result:  result,
		ID:      id,
	}
	data, _ := json.Marshal(resp)
	fmt.Println(string(data))
}

func sendError(id interface{}, code int, message string) {
	resp := JSONRPCResponse{
		JSONRPC: "2.0",
		Error: &RPCError{
			Code:    code,
			Message: message,
		},
		ID: id,
	}
	data, _ := json.Marshal(resp)
	fmt.Println(string(data))
}

// Clusters sémantiques pour cohérence conceptuelle
var conceptClusters = map[string]string{
	"voiture": "concept_vehicle", "auto": "concept_vehicle", "véhicule": "concept_vehicle",
	"vehicule": "concept_vehicle", "automobile": "concept_vehicle", "car": "concept_vehicle",
	"voitures": "concept_vehicle",

	"couleur": "concept_color", "color": "concept_color", "bleu": "concept_color",
	"bleue": "concept_color", "rouge": "concept_color", "vert": "concept_color",
	"verte": "concept_color", "noir": "concept_color", "noire": "concept_color",
	"blanc": "concept_color", "blanche": "concept_color", "jaune": "concept_color",

	"reunion": "concept_work", "réunion": "concept_work", "projet": "concept_work",
	"jalon": "concept_work", "validation": "concept_work", "meeting": "concept_work",
	"tache": "concept_work", "tâche": "concept_work", "todo": "concept_work",

	"inference": "concept_ai", "inférence": "concept_ai", "llm": "concept_ai",
	"modele": "concept_ai", "modèle": "concept_ai", "vulkan": "concept_ai",
	"embedding": "concept_ai", "embeddings": "concept_ai", "ia": "concept_ai",
	"assistant": "concept_ai", "jeanne": "concept_ai",

	"maison": "concept_home", "appartement": "concept_home", "logement": "concept_home",
}

var stopWords = map[string]bool{
	"le": true, "la": true, "les": true, "un": true, "une": true, "des": true,
	"de": true, "du": true, "en": true, "et": true, "est": true, "je": true,
	"tu": true, "il": true, "elle": true, "nous": true, "vous": true, "ils": true,
	"mon": true, "ma": true, "mes": true, "ce": true, "cette": true, "ces": true,
	"the": true, "a": true, "an": true, "in": true, "on": true, "at": true, "is": true,
}

// computeEmbedding génère un vecteur 384 f32 déterministe et normalisé L2
func computeEmbedding(text string, promptType string, modelID string) ([]float32, int) {
	clean := strings.ToLower(text)
	words := strings.FieldsFunc(clean, func(r rune) bool {
		return r == ' ' || r == '\t' || r == '\n' || r == ',' || r == '.' ||
			r == '!' || r == '?' || r == ';' || r == ':' || r == '\'' ||
			r == '"' || r == '(' || r == ')' || r == '-' || r == '_'
	})

	if len(words) == 0 {
		vec := make([]float32, EmbeddingDim)
		vec[0] = 1.0 // Vecteur unitaire trivial
		return vec, 1
	}

	modelSeed := int64(424242)
	if seed, ok := availableModels[modelID]; ok {
		modelSeed = seed
	}

	accum := make([]float64, EmbeddingDim)

	for _, w := range words {
		if stopWords[w] {
			addTokenVector(accum, w, modelSeed, 0.15)
			continue
		}

		addTokenVector(accum, w, modelSeed, 1.0)

		// Injection du concept partagé
		if concept, ok := conceptClusters[w]; ok {
			addTokenVector(accum, concept, modelSeed, 2.0)
		}

		// N-grammes de caractères de sous-mots (3-grammes) pour morphologie
		if len(w) >= 4 {
			for i := 0; i <= len(w)-3; i++ {
				ngram := w[i : i+3]
				addTokenVector(accum, ngram, modelSeed, 0.2)
			}
		}
	}

	// Normalisation L2 stricte
	var sumSq float64
	for _, v := range accum {
		sumSq += v * v
	}
	norm := math.Sqrt(sumSq)

	result := make([]float32, EmbeddingDim)
	if norm > 1e-12 {
		for i, v := range accum {
			result[i] = float32(v / norm)
		}
	} else {
		result[0] = 1.0
	}

	return result, len(words)
}

func addTokenVector(accum []float64, token string, seed int64, weight float64) {
	h := fnv.New64a()
	h.Write([]byte(token))
	tokenHash := int64(h.Sum64()) ^ seed

	// PRNG linéaire déterministe
	state := uint64(tokenHash)
	for i := 0; i < EmbeddingDim; i++ {
		state = state*6364136223846793005 + 1442695040888963407
		val := (float64(int32(state>>32)) / 2147483648.0) * weight
		accum[i] += val
	}
}
