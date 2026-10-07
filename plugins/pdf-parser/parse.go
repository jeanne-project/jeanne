package main

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"rsc.io/pdf"
)

// Codes d'erreur JSON-RPC normalisés (spec 07 §2.4).
const (
	codeParseError    = -32700
	codeMethodUnknown = -32601
	codeInvalidParams = -32602
	codeFileNotFound  = -32001
	codeCorrupted     = -32002
	codeTimeout       = -32003
)

type rpcError struct {
	Code    int    `json:"code"`
	Message string `json:"message"`
}

func (e *rpcError) Error() string { return e.Message }

type parseOptions struct {
	ExtractTables bool `json:"extract_tables"`
	ExtractImages bool `json:"extract_images"`
}

type parseResult struct {
	Title           string         `json:"title"`
	ContentMarkdown string         `json:"content_markdown"`
	Metadata        map[string]any `json:"metadata"`
	Attachments     []any          `json:"attachments"`
}

// line est une ligne visuelle reconstituée d'une page.
type line struct {
	y     float64
	size  float64
	cells []string
	gaps  []bool // gaps[i] = true si un grand espace précède cells[i] (i>0)
}

// parsePDF ouvre le fichier en streaming (lecture disque via ReaderAt) et produit du Markdown.
func parsePDF(path string, opts parseOptions) (res *parseResult, err error) {
	if path == "" || !filepath.IsAbs(path) {
		return nil, &rpcError{codeInvalidParams, "file_path absolu requis"}
	}
	st, statErr := os.Stat(path)
	if statErr != nil {
		return nil, &rpcError{codeFileNotFound, fmt.Sprintf("fichier introuvable ou inaccessible: %v", statErr)}
	}
	if st.IsDir() {
		return nil, &rpcError{codeInvalidParams, "file_path désigne un répertoire"}
	}

	// rsc.io/pdf signale ses erreurs par panic : on les convertit en erreur structurée.
	defer func() {
		if r := recover(); r != nil {
			res = nil
			err = &rpcError{codeCorrupted, fmt.Sprintf("document PDF corrompu ou chiffré: %v", r)}
		}
	}()

	f, openErr := os.Open(path)
	if openErr != nil {
		return nil, &rpcError{codeFileNotFound, fmt.Sprintf("permission refusée: %v", openErr)}
	}
	defer f.Close()

	rd, rdErr := pdf.NewReaderEncrypted(f, st.Size(), func() string { return "" })
	if rdErr != nil {
		return nil, &rpcError{codeCorrupted, fmt.Sprintf("document PDF illisible ou chiffré: %v", rdErr)}
	}

	meta := extractMetadata(rd)
	pages := rd.NumPage()
	meta["page_count"] = pages

	var md strings.Builder
	for p := 1; p <= pages; p++ {
		page := rd.Page(p)
		if page.V.IsNull() {
			continue
		}
		lines := buildLines(page.Content().Text)
		renderPage(&md, lines, opts.ExtractTables)
	}

	title, _ := meta["title"].(string)
	content := strings.TrimSpace(md.String())
	if title == "" {
		title = firstHeading(content)
	}
	if title == "" {
		title = strings.TrimSuffix(filepath.Base(path), filepath.Ext(path))
	}
	delete(meta, "title")
	return &parseResult{
		Title:           title,
		ContentMarkdown: content + "\n",
		Metadata:        meta,
		Attachments:     []any{},
	}, nil
}

func extractMetadata(rd *pdf.Reader) map[string]any {
	meta := map[string]any{}
	info := rd.Trailer().Key("Info")
	if info.IsNull() {
		return meta
	}
	if t := strings.TrimSpace(info.Key("Title").Text()); t != "" {
		meta["title"] = t
	}
	if a := strings.TrimSpace(info.Key("Author").Text()); a != "" {
		meta["author"] = a
	}
	if d := normalizeDate(info.Key("CreationDate").Text()); d != "" {
		meta["date"] = d
	}
	return meta
}

// normalizeDate convertit "D:20260315120000Z" en "2026-03-15".
func normalizeDate(raw string) string {
	raw = strings.TrimPrefix(strings.TrimSpace(raw), "D:")
	if len(raw) < 8 {
		return ""
	}
	for _, c := range raw[:8] {
		if c < '0' || c > '9' {
			return ""
		}
	}
	return raw[:4] + "-" + raw[4:6] + "-" + raw[6:8]
}

// buildLines regroupe les fragments de texte par ordonnée puis par abscisse.
func buildLines(texts []pdf.Text) []line {
	if len(texts) == 0 {
		return nil
	}
	sorted := make([]pdf.Text, len(texts))
	copy(sorted, texts)
	sort.SliceStable(sorted, func(i, j int) bool {
		if abs(sorted[i].Y-sorted[j].Y) > 2 {
			return sorted[i].Y > sorted[j].Y // haut de page d'abord
		}
		return sorted[i].X < sorted[j].X
	})

	var lines []line
	var cur *line
	var prevEnd float64
	for _, t := range sorted {
		if cur == nil && strings.TrimSpace(t.S) == "" {
			continue
		}
		if cur != nil && abs(cur.y-t.Y) <= 2 && strings.TrimSpace(t.S) == "" {
			// Les espaces sont conservés tels quels dans la cellule courante.
			cur.cells[len(cur.cells)-1] += t.S
			prevEnd = t.X + t.W
			continue
		}
		if cur == nil || abs(cur.y-t.Y) > 2 {
			lines = append(lines, line{y: t.Y, size: t.FontSize})
			cur = &lines[len(lines)-1]
			cur.cells = []string{t.S}
			cur.gaps = []bool{false}
			prevEnd = t.X + t.W
			continue
		}
		if t.FontSize > cur.size {
			cur.size = t.FontSize
		}
		gap := t.X - prevEnd
		last := len(cur.cells) - 1
		switch {
		case gap > 1.5*t.FontSize:
			cur.cells = append(cur.cells, t.S)
			cur.gaps = append(cur.gaps, true)
		case gap > 0.2*t.FontSize && !strings.HasSuffix(cur.cells[last], " "):
			cur.cells[last] += " " + t.S
		default:
			cur.cells[last] += t.S
		}
		prevEnd = t.X + t.W
	}
	for i := range lines {
		lines[i].trim()
	}
	return lines
}

// trim supprime les espaces de bordure des cellules et les cellules vides.
func (l *line) trim() {
	var cells []string
	var gaps []bool
	for i, c := range l.cells {
		if c = strings.TrimSpace(c); c != "" {
			cells = append(cells, c)
			gaps = append(gaps, l.gaps[i])
		}
	}
	l.cells, l.gaps = cells, gaps
}

func abs(f float64) float64 {
	if f < 0 {
		return -f
	}
	return f
}

func bodySize(lines []line) float64 {
	count := map[float64]int{}
	best, bestN := 0.0, 0
	for _, l := range lines {
		n := 0
		for _, c := range l.cells {
			n += len(c)
		}
		count[l.size] += n
		if count[l.size] > bestN {
			best, bestN = l.size, count[l.size]
		}
	}
	return best
}

func renderPage(md *strings.Builder, lines []line, tables bool) {
	body := bodySize(lines)
	for i := 0; i < len(lines); {
		l := lines[i]
		if len(l.cells) == 0 {
			i++
			continue
		}
		if tables && len(l.cells) >= 2 {
			j := i
			for j < len(lines) && len(lines[j].cells) == len(l.cells) {
				j++
			}
			if j-i >= 2 {
				renderTable(md, lines[i:j])
				i = j
				continue
			}
		}
		text := strings.Join(l.cells, " ")
		switch {
		case body > 0 && l.size >= body*1.5:
			md.WriteString("# " + text + "\n\n")
		case body > 0 && l.size >= body*1.2:
			md.WriteString("## " + text + "\n\n")
		default:
			md.WriteString(text + "\n")
		}
		i++
	}
	md.WriteString("\n")
}

func renderTable(md *strings.Builder, rows []line) {
	md.WriteString("\n")
	for idx, r := range rows {
		md.WriteString("| " + strings.Join(escapeCells(r.cells), " | ") + " |\n")
		if idx == 0 {
			md.WriteString("|" + strings.Repeat(" --- |", len(r.cells)) + "\n")
		}
	}
	md.WriteString("\n")
}

func escapeCells(cells []string) []string {
	out := make([]string, len(cells))
	for i, c := range cells {
		out[i] = strings.ReplaceAll(c, "|", "\\|")
	}
	return out
}

func firstHeading(md string) string {
	for _, l := range strings.Split(md, "\n") {
		if strings.HasPrefix(l, "# ") {
			return strings.TrimSpace(l[2:])
		}
	}
	return ""
}
