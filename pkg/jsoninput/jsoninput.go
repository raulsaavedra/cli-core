package jsoninput

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"

	"github.com/raulsaavedra/cli-core/pkg/stdio"
)

type ReadOptions struct {
	Data       string
	File       string
	Label      string
	AllowEmpty bool
}

func Read(opts ReadOptions) (json.RawMessage, error) {
	label := opts.Label
	if label == "" {
		label = "JSON input"
	}

	var raw string
	switch {
	case opts.Data != "" && opts.File != "":
		return nil, fmt.Errorf("%s: use either --data or --file", label)
	case opts.Data != "":
		raw = opts.Data
	case opts.File != "":
		b, err := os.ReadFile(opts.File)
		if err != nil {
			return nil, fmt.Errorf("%s: read %s: %w", label, opts.File, err)
		}
		raw = string(b)
	default:
		stdinData, err := stdio.ReadStdin()
		if err != nil {
			return nil, fmt.Errorf("%s: read stdin: %w", label, err)
		}
		raw = stdinData
	}

	raw = strings.TrimSpace(raw)
	if raw == "" && opts.AllowEmpty {
		return json.RawMessage("null"), nil
	}
	if raw == "" {
		return nil, fmt.Errorf("%s: empty payload", label)
	}

	var out json.RawMessage
	if err := json.Unmarshal([]byte(raw), &out); err != nil {
		return nil, fmt.Errorf("%s: invalid JSON: %w", label, err)
	}
	return out, nil
}
