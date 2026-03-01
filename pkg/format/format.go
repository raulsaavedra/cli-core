package format

import (
	"strings"
	"time"
)

func Date(value string) string {
	return formatWith(value, "2006-01-02")
}

func DateTime(value string) string {
	return formatWith(value, "2006-01-02 15:04:05")
}

func Truncate(text string, max int) string {
	if max <= 0 || len(text) <= max {
		return text
	}
	if max <= 3 {
		return text[:max]
	}
	return strings.TrimSpace(text[:max-3]) + "..."
}

func formatWith(value, layout string) string {
	if value == "" {
		return value
	}
	t, err := time.Parse(time.RFC3339, value)
	if err != nil {
		return value
	}
	return t.Format(layout)
}
