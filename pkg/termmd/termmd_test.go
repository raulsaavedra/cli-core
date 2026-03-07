package termmd

import (
	"strings"
	"testing"
)

func TestRenderProducesPlainAndRenderedLines(t *testing.T) {
	result := Render("# Title\n\nParagraph with **bold** text and [a link](https://example.com).", 60)

	if len(result.Lines) == 0 {
		t.Fatal("expected rendered lines")
	}
	if len(result.Plain) != len(result.Lines) {
		t.Fatalf("expected plain and rendered line counts to match, got %d and %d", len(result.Plain), len(result.Lines))
	}

	joined := strings.Join(result.Plain, "\n")
	if !strings.Contains(joined, "Title") {
		t.Fatalf("expected heading text in plain output, got %q", joined)
	}
	if !strings.Contains(joined, "Paragraph with bold text and a link https://example.com.") {
		t.Fatalf("expected renderer output in plain text, got %q", joined)
	}
	if len(result.Headings) != 1 {
		t.Fatalf("expected one heading, got %d", len(result.Headings))
	}
	if result.Headings[0].Text != "Title" {
		t.Fatalf("expected heading text Title, got %q", result.Headings[0].Text)
	}
	if len(result.Links) != 1 {
		t.Fatalf("expected one link, got %d", len(result.Links))
	}
	if result.Links[0].Href != "https://example.com" {
		t.Fatalf("expected example.com link, got %q", result.Links[0].Href)
	}
}

func TestRenderClampsSmallWidth(t *testing.T) {
	result := Render("hello", 1)
	if len(result.Lines) == 0 {
		t.Fatal("expected lines for tiny width input")
	}
}

func TestRenderUsesTerminalDefaultForegroundForBody(t *testing.T) {
	result := Render("Paragraph with `code` and [a link](https://example.com).", 80)

	if strings.Contains(result.Rendered, "\x1b[38;5;252m") {
		t.Fatalf("expected body text to use terminal default foreground, got %q", result.Rendered)
	}
	if strings.Contains(result.Rendered, "\x1b[38;5;203m") {
		t.Fatalf("expected inline code to avoid custom foreground override, got %q", result.Rendered)
	}
}

func TestRenderCompactsHeadingAndCodeBlockSpacing(t *testing.T) {
	result := Render("## Heading\n\n```ts\nconst x = 1\n```\n\nAfter", 80)
	joined := strings.Join(result.Plain, "\n")

	if strings.Contains(joined, "## Heading\n\n\n") {
		t.Fatalf("expected no extra blank run after heading, got %q", joined)
	}
	if strings.Contains(joined, "const x = 1\n\n\nAfter") {
		t.Fatalf("expected no extra blank run after code block, got %q", joined)
	}
}

func TestNormalizeRenderedLinesRemovesSharedIndentAndOuterBlankLines(t *testing.T) {
	got := normalizeRenderedLines([]string{
		"",
		"  # Heading",
		"  body",
		"",
	})

	want := []string{
		"# Heading",
		"body",
	}
	if strings.Join(got, "\n") != strings.Join(want, "\n") {
		t.Fatalf("unexpected normalized lines: got %q want %q", got, want)
	}
}

func TestNormalizeRenderedLinesCollapsesBlankRuns(t *testing.T) {
	got := normalizeRenderedLines([]string{
		"  one",
		"",
		"",
		"",
		"  two",
	})

	want := []string{
		"one",
		"",
		"two",
	}
	if strings.Join(got, "\n") != strings.Join(want, "\n") {
		t.Fatalf("unexpected blank-run normalization: got %q want %q", got, want)
	}
}
