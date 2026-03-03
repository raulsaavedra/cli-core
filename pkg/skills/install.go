package skills

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
)

type InstallOptions struct {
	SrcDir    string
	DestDir   string
	Name      string
	Overwrite bool
	Link      bool
}

func ResolveSkillsDir(dest string) (string, error) {
	if dest != "" {
		return filepath.Abs(dest)
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", fmt.Errorf("resolve home dir: %w", err)
	}
	return filepath.Join(home, ".agents", "skills"), nil
}

func Install(opts InstallOptions) (string, error) {
	if opts.SrcDir == "" {
		return "", fmt.Errorf("source directory is required")
	}
	if opts.DestDir == "" {
		return "", fmt.Errorf("destination directory is required")
	}
	name := opts.Name
	if name == "" {
		name = filepath.Base(opts.SrcDir)
	}

	src, err := filepath.Abs(opts.SrcDir)
	if err != nil {
		return "", err
	}
	destRoot, err := filepath.Abs(opts.DestDir)
	if err != nil {
		return "", err
	}
	dest := filepath.Join(destRoot, name)

	if err := os.MkdirAll(destRoot, 0o755); err != nil {
		return "", fmt.Errorf("create destination directory: %w", err)
	}

	if _, err := os.Stat(dest); err == nil {
		if !opts.Overwrite {
			return "", fmt.Errorf("destination exists: %s", dest)
		}
		if rmErr := os.RemoveAll(dest); rmErr != nil {
			return "", fmt.Errorf("remove existing destination: %w", rmErr)
		}
	}

	if opts.Link {
		if err := os.Symlink(src, dest); err != nil {
			return "", fmt.Errorf("create symlink: %w", err)
		}
		return dest, nil
	}

	if err := copyDir(src, dest); err != nil {
		return "", err
	}
	return dest, nil
}

func copyDir(src, dest string) error {
	entries, err := os.ReadDir(src)
	if err != nil {
		return fmt.Errorf("read source directory: %w", err)
	}
	if err := os.MkdirAll(dest, 0o755); err != nil {
		return fmt.Errorf("create destination directory: %w", err)
	}
	for _, entry := range entries {
		srcPath := filepath.Join(src, entry.Name())
		destPath := filepath.Join(dest, entry.Name())
		if entry.IsDir() {
			if err := copyDir(srcPath, destPath); err != nil {
				return err
			}
			continue
		}
		if err := copyFile(srcPath, destPath); err != nil {
			return err
		}
	}
	return nil
}

func copyFile(src, dest string) error {
	in, err := os.Open(src)
	if err != nil {
		return fmt.Errorf("open source file: %w", err)
	}
	defer in.Close()

	info, err := in.Stat()
	if err != nil {
		return fmt.Errorf("stat source file: %w", err)
	}

	out, err := os.OpenFile(dest, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, info.Mode())
	if err != nil {
		return fmt.Errorf("open destination file: %w", err)
	}
	defer out.Close()

	if _, err := io.Copy(out, in); err != nil {
		return fmt.Errorf("copy file: %w", err)
	}
	return nil
}
