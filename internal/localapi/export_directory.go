package api

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"os"
	"path/filepath"

	"github.com/uniclipboard/unic/internal/directory"
)

// exportDirectory creates independent user-owned roots, including equal names.
// Only containers created by this call are rollback targets.
func (c *Client) exportDirectory(ctx context.Context, item HistoryItem, dir string) (names []string, err error) {
	var files []int
	var sizes []int64
	manifestPart := -1
	for i, part := range item.Parts {
		if part.Kind == "file" {
			files = append(files, i)
			sizes = append(sizes, part.Size)
		}
		if part.MIME == directory.MIME {
			if manifestPart >= 0 {
				return nil, fmt.Errorf("duplicate directory manifest")
			}
			manifestPart = i
		}
	}
	if manifestPart < 0 {
		return nil, fmt.Errorf("missing directory manifest")
	}
	content, err := c.HistoryContent(ctx, item.ID, manifestPart)
	if err != nil {
		return nil, err
	}
	data, err := io.ReadAll(io.LimitReader(content, directory.MaxManifest+1))
	_ = content.Close()
	if err != nil {
		return nil, err
	}
	manifest, err := directory.Decode(data, sizes)
	if err != nil {
		return nil, err
	}
	if err = os.MkdirAll(dir, 0700); err != nil {
		return nil, err
	}
	root, err := os.OpenRoot(dir)
	if err != nil {
		return nil, err
	}
	defer func() { _ = root.Close() }()
	var owned []string
	defer func() {
		if err != nil {
			for _, name := range owned {
				if cleanupErr := root.RemoveAll(name); cleanupErr != nil {
					err = fmt.Errorf("directory export failed; cannot remove incomplete root %s: %w", name, cleanupErr)
				}
			}
		}
	}()
	for i, selected := range manifest.Roots {
		container := fmt.Sprintf("r%d", i)
		if err = root.Mkdir(container, 0700); err != nil {
			return nil, err
		}
		owned = append(owned, container)
		names = append(names, filepath.Join(container, selected.Name))
	}
	for _, entry := range manifest.Entries {
		if err = ctx.Err(); err != nil {
			return nil, err
		}
		name := filepath.Join(names[entry.Root], filepath.FromSlash(entry.Path))
		if entry.Directory {
			if err = root.Mkdir(name, 0700); err != nil {
				return nil, err
			}
			continue
		}
		if err = c.exportDirectoryFile(ctx, item.ID, files[entry.File], root, name, entry); err != nil {
			return nil, err
		}
	}
	return names, nil
}

func (c *Client) exportDirectoryFile(ctx context.Context, id string, part int, root *os.Root, name string, entry directory.Entry) error {
	content, err := c.HistoryContent(ctx, id, part)
	if err != nil {
		return err
	}
	defer func() { _ = content.Close() }()
	file, err := root.OpenFile(name, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return err
	}
	defer func() { _ = file.Close() }()
	hash := sha256.New()
	n, err := io.Copy(io.MultiWriter(file, hash), io.LimitReader(content, entry.Size+1))
	if err != nil {
		return err
	}
	if n != entry.Size || hex.EncodeToString(hash.Sum(nil)) != entry.SHA256 {
		return fmt.Errorf("directory export integrity failed")
	}
	if err = file.Sync(); err != nil {
		return err
	}
	return file.Close()
}
