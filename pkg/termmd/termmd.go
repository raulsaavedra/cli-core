package termmd

import (
	"regexp"
	"strings"
	"unicode/utf8"

	"github.com/charmbracelet/glamour"
	glamouransi "github.com/charmbracelet/glamour/ansi"
	styles "github.com/charmbracelet/glamour/styles"
	"github.com/charmbracelet/x/ansi"
	"github.com/yuin/goldmark"
	"github.com/yuin/goldmark/ast"
	"github.com/yuin/goldmark/extension"
	goldtext "github.com/yuin/goldmark/text"
)

var ansiRegex = regexp.MustCompile(`\x1b\[[0-9;]*m`)

type Heading struct {
	Level int
	Text  string
	Line  int
}

type Link struct {
	Text string
	Href string
}

type Result struct {
	Rendered string
	Lines    []string
	Plain    []string
	Headings []Heading
	Links    []Link
}

func Render(content string, width int) Result {
	rawHeadings, allLinks := extractMarkdownMetadata(content)

	if width < 20 {
		width = 20
	}

	rendered := content
	renderer, err := glamour.NewTermRenderer(
		glamour.WithStyles(sharedStyleConfig()),
		glamour.WithWordWrap(width),
	)
	if err == nil {
		if out, renderErr := renderer.Render(content); renderErr == nil {
			rendered = out
		}
	}

	lines := normalizeRenderedLines(strings.Split(strings.TrimRight(rendered, "\n"), "\n"))
	for i, line := range lines {
		if ansi.StringWidth(line) > width {
			lines[i] = ansi.Truncate(line, width, "")
		}
	}
	plain := make([]string, 0, len(lines))
	for _, line := range lines {
		plain = append(plain, strings.TrimRight(stripANSI(line), " "))
	}

	headings := make([]Heading, 0, len(rawHeadings))
	searchStart := 0
	for _, h := range rawHeadings {
		lineIndex := 0
		needle := strings.ToLower(strings.TrimSpace(h.Text))
		if needle != "" {
			found := false
			for i := searchStart; i < len(plain); i++ {
				if strings.Contains(strings.ToLower(plain[i]), needle) {
					lineIndex = i
					searchStart = i + 1
					found = true
					break
				}
			}
			if !found && len(plain) > 0 {
				lineIndex = len(plain) - 1
			}
		}
		headings = append(headings, Heading{
			Level: h.Level,
			Text:  h.Text,
			Line:  lineIndex,
		})
	}

	return Result{
		Rendered: strings.Join(lines, "\n"),
		Lines:    lines,
		Plain:    plain,
		Headings: headings,
		Links:    allLinks,
	}
}

func sharedStyleConfig() glamouransi.StyleConfig {
	cfg := styles.DarkStyleConfig
	noMargin := uint(0)
	cfg.Document.Margin = &noMargin
	cfg.Document.StylePrimitive.Color = nil
	cfg.Paragraph.StylePrimitive.Color = nil
	cfg.Text.Color = nil
	cfg.LinkText.Color = nil
	cfg.Code.StylePrimitive.Color = nil
	return cfg
}

func normalizeRenderedLines(lines []string) []string {
	if len(lines) == 0 {
		return []string{""}
	}

	lines = trimOuterBlankLines(lines)
	if len(lines) == 0 {
		return []string{""}
	}

	commonIndent := commonLeadingIndent(lines)
	if commonIndent > 0 {
		normalized := make([]string, 0, len(lines))
		for _, line := range lines {
			normalized = append(normalized, trimLeadingIndent(line, commonIndent))
		}
		lines = normalized
	}

	lines = collapseBlankRuns(lines, 1)
	if len(lines) == 0 {
		return []string{""}
	}
	return lines
}

func trimOuterBlankLines(lines []string) []string {
	start := 0
	for start < len(lines) && isBlankLine(lines[start]) {
		start++
	}
	end := len(lines)
	for end > start && isBlankLine(lines[end-1]) {
		end--
	}
	return lines[start:end]
}

func commonLeadingIndent(lines []string) int {
	common := -1
	for _, line := range lines {
		if isBlankLine(line) {
			continue
		}
		indent := leadingIndentWidth(line)
		if common == -1 || indent < common {
			common = indent
		}
	}
	if common < 0 {
		return 0
	}
	return common
}

func leadingIndentWidth(line string) int {
	width := 0
	for len(line) > 0 {
		if _, rest, ok := cutANSIPrefix(line); ok {
			line = rest
			continue
		}
		r, size := utf8.DecodeRuneInString(line)
		if r == ' ' || r == '\t' {
			width++
			line = line[size:]
			continue
		}
		break
	}
	return width
}

func trimLeadingIndent(line string, width int) string {
	if width <= 0 || line == "" {
		return line
	}
	var b strings.Builder
	trimmed := 0
	for len(line) > 0 {
		if seq, rest, ok := cutANSIPrefix(line); ok {
			b.WriteString(seq)
			line = rest
			continue
		}
		r, size := utf8.DecodeRuneInString(line)
		if trimmed >= width {
			b.WriteString(line)
			return b.String()
		}
		if r != ' ' && r != '\t' {
			b.WriteString(line)
			return b.String()
		}
		trimmed++
		line = line[size:]
	}
	return b.String()
}

func collapseBlankRuns(lines []string, keep int) []string {
	if keep < 0 {
		keep = 0
	}
	out := make([]string, 0, len(lines))
	blankRun := 0
	for _, line := range lines {
		if isBlankLine(line) {
			blankRun++
			if blankRun <= keep {
				out = append(out, "")
			}
			continue
		}
		blankRun = 0
		out = append(out, line)
	}
	return out
}

func isBlankLine(line string) bool {
	return strings.TrimSpace(stripANSI(line)) == ""
}

func stripANSI(line string) string {
	return ansiRegex.ReplaceAllString(line, "")
}

func cutANSIPrefix(line string) (string, string, bool) {
	if line == "" || line[0] != '\x1b' {
		return "", line, false
	}
	loc := ansiRegex.FindStringIndex(line)
	if loc == nil || loc[0] != 0 {
		return "", line, false
	}
	return line[:loc[1]], line[loc[1]:], true
}

func extractMarkdownMetadata(content string) ([]Heading, []Link) {
	source := []byte(content)
	parser := goldmark.New(goldmark.WithExtensions(extension.GFM))
	doc := parser.Parser().Parse(goldtext.NewReader(source))

	rawHeadings := []Heading{}
	allLinks := []Link{}
	seenLinks := map[string]struct{}{}

	_ = ast.Walk(doc, func(node ast.Node, entering bool) (ast.WalkStatus, error) {
		if !entering {
			return ast.WalkContinue, nil
		}
		switch n := node.(type) {
		case *ast.Heading:
			text := extractNodeText(n, source)
			if text == "" {
				return ast.WalkContinue, nil
			}
			rawHeadings = append(rawHeadings, Heading{
				Level: n.Level,
				Text:  text,
			})
		case *ast.Link:
			href := strings.TrimSpace(string(n.Destination))
			text := extractNodeText(n, source)
			appendUniqueLink(&allLinks, seenLinks, text, href)
		case *ast.AutoLink:
			href := strings.TrimSpace(string(n.URL(source)))
			text := normalizeSpaces(string(n.Label(source)))
			appendUniqueLink(&allLinks, seenLinks, text, href)
		}
		return ast.WalkContinue, nil
	})

	return rawHeadings, allLinks
}

func appendUniqueLink(allLinks *[]Link, seen map[string]struct{}, text, href string) {
	text = strings.TrimSpace(text)
	href = strings.TrimSpace(href)
	if href == "" {
		return
	}
	if text == "" {
		text = href
	}
	key := text + "|" + href
	if _, exists := seen[key]; exists {
		return
	}
	seen[key] = struct{}{}
	*allLinks = append(*allLinks, Link{
		Text: text,
		Href: href,
	})
}

func extractNodeText(node ast.Node, source []byte) string {
	return normalizeSpaces(string(node.Text(source)))
}

func normalizeSpaces(value string) string {
	return strings.Join(strings.Fields(value), " ")
}
