package stdio

import (
	"io"
	"os"
	"strings"
)

func ReadStdin() (string, error) {
	data, err := io.ReadAll(os.Stdin)
	if err != nil {
		return "", err
	}
	return strings.TrimSpace(string(data)), nil
}
