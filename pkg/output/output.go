package output

import (
	"encoding/json"
	"fmt"
	"os"
)

func JSON(v any) error {
	enc := json.NewEncoder(os.Stdout)
	enc.SetIndent("", "  ")
	return enc.Encode(v)
}

func Success(format string, args ...any) {
	fmt.Printf(format+"\n", args...)
}

func Errorf(format string, args ...any) {
	fmt.Fprintf(os.Stderr, format+"\n", args...)
}
