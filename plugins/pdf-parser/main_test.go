package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

type textOp struct {
	size float64
	x, y float64
	s    string
}

// buildPDF assemble un PDF minimal valide (Helvetica) avec table xref calculée.
func buildPDF(ops []textOp, info string) []byte {
	var stream bytes.Buffer
	stream.WriteString("BT\n")
	for _, o := range ops {
		fmt.Fprintf(&stream, "/F1 %g Tf 1 0 0 1 %g %g Tm (%s) Tj\n", o.size, o.x, o.y, o.s)
	}
	stream.WriteString("ET\n")
	objs := []string{
		"<< /Type /Catalog /Pages 2 0 R >>",
		"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
		"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
		fmt.Sprintf("<< /Length %d >>\nstream\n%sendstream", stream.Len(), stream.String()),
		"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 32 /LastChar 126 /Widths [" + helvWidths() + "] >>",
		info,
	}
	var b bytes.Buffer
	b.WriteString("%PDF-1.4\n")
	offs := make([]int, len(objs))
	for i, o := range objs {
		offs[i] = b.Len()
		fmt.Fprintf(&b, "%d 0 obj\n%s\nendobj\n", i+1, o)
	}
	xref := b.Len()
	fmt.Fprintf(&b, "xref\n0 %d\n0000000000 65535 f \n", len(objs)+1)
	for _, o := range offs {
		fmt.Fprintf(&b, "%010d 00000 n \n", o)
	}
	fmt.Fprintf(&b, "trailer\n<< /Size %d /Root 1 0 R /Info 6 0 R >>\nstartxref\n%d\n%%%%EOF\n", len(objs)+1, xref)
	return b.Bytes()
}

func samplePDF(t *testing.T) string {
	t.Helper()
	ops := []textOp{
		{24, 72, 720, "Financial Report 2026"},
		{12, 72, 690, "Revenue increased by 14 percent."},
		{12, 72, 670, "Second body line of the summary."},
		{16, 72, 640, "Key Figures"},
		{12, 72, 610, "Region"}, {12, 250, 610, "Revenue"},
		{12, 72, 595, "Europe"}, {12, 250, 595, "120"},
		{12, 72, 580, "Asia"}, {12, 250, 580, "95"},
	}
	info := "<< /Title (Financial Report 2026) /Author (CFO Office) /CreationDate (D:20260315120000Z) >>"
	p := filepath.Join(t.TempDir(), "report.pdf")
	if err := os.WriteFile(p, buildPDF(ops, info), 0o600); err != nil {
		t.Fatal(err)
	}
	return p
}

func TestParseSamplePDF(t *testing.T) {
	res, err := parsePDF(samplePDF(t), parseOptions{ExtractTables: true})
	if err != nil {
		t.Fatalf("erreur inattendue: %v", err)
	}
	if res.Title != "Financial Report 2026" {
		t.Errorf("titre = %q", res.Title)
	}
	md := res.ContentMarkdown
	for _, want := range []string{"# Financial Report 2026", "## Key Figures", "| Region | Revenue |", "| --- | --- |", "| Europe | 120 |"} {
		if !strings.Contains(md, want) {
			t.Errorf("markdown sans %q:\n%s", want, md)
		}
	}
	if res.Metadata["author"] != "CFO Office" || res.Metadata["date"] != "2026-03-15" || res.Metadata["page_count"] != 1 {
		t.Errorf("métadonnées = %v", res.Metadata)
	}
}

func TestErrorCodes(t *testing.T) {
	if _, err := parsePDF("/nonexistent/x.pdf", parseOptions{}); err == nil || err.(*rpcError).Code != codeFileNotFound {
		t.Errorf("attendu -32001, obtenu %v", err)
	}
	if _, err := parsePDF("relative.pdf", parseOptions{}); err == nil || err.(*rpcError).Code != codeInvalidParams {
		t.Errorf("attendu -32602, obtenu %v", err)
	}
	bad := filepath.Join(t.TempDir(), "bad.pdf")
	_ = os.WriteFile(bad, []byte("not a pdf at all"), 0o600)
	if _, err := parsePDF(bad, parseOptions{}); err == nil || err.(*rpcError).Code != codeCorrupted {
		t.Errorf("attendu -32002, obtenu %v", err)
	}
}

func TestRunProtocol(t *testing.T) {
	path := samplePDF(t)
	in := strings.NewReader("{broken\n" +
		`{"jsonrpc":"2.0","method":"nope","params":{},"id":5}` + "\n" +
		fmt.Sprintf(`{"jsonrpc":"2.0","method":"parse_document","params":{"file_path":%q,"options":{"extract_tables":true}},"id":7}`, path) + "\n")
	var out bytes.Buffer
	run(in, &out)
	lines := strings.Split(strings.TrimSpace(out.String()), "\n")
	if len(lines) != 3 {
		t.Fatalf("3 réponses attendues, obtenu %d: %s", len(lines), out.String())
	}
	var r [3]response
	raw := make([]map[string]any, 3)
	for i, l := range lines {
		_ = json.Unmarshal([]byte(l), &raw[i])
		_ = json.Unmarshal([]byte(l), &r[i])
	}
	if r[0].Error == nil || r[0].Error.Code != codeParseError {
		t.Errorf("parse error attendu: %s", lines[0])
	}
	if r[1].Error == nil || r[1].Error.Code != codeMethodUnknown || r[1].ID != 5 {
		t.Errorf("method not found attendu: %s", lines[1])
	}
	if r[2].Error != nil || r[2].ID != 7 || raw[2]["result"] == nil {
		t.Errorf("résultat attendu: %s", lines[2])
	}
}

// helvWidths renvoie des largeurs approximatives (0.5 em = 500) pour les glyphes 32..126.
func helvWidths() string {
	w := make([]string, 0, 95)
	for i := 32; i <= 126; i++ {
		if i == 32 {
			w = append(w, "278")
		} else {
			w = append(w, "556")
		}
	}
	return strings.Join(w, " ")
}
