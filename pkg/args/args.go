package args

import (
	"fmt"
	"strconv"
)

func ParseIntOption(value, label string) (int, error) {
	n, err := strconv.Atoi(value)
	if err != nil {
		return 0, fmt.Errorf("%s must be an integer", label)
	}
	return n, nil
}

func RequireExactlyOne(values map[string]bool, message string) error {
	count := 0
	for _, present := range values {
		if present {
			count++
		}
	}
	if count != 1 {
		return fmt.Errorf("%s", message)
	}
	return nil
}
