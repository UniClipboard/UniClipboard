package main

import (
	"context"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"time"

	"github.com/uniclipboard/unic/engine"
)

// Independent filesystem inventories do not trust the product manifest as an oracle.
type treeMember struct {
	Path      string `json:"path"`
	Directory bool   `json:"directory"`
	Size      int64  `json:"size"`
	SHA       string `json:"sha256,omitempty"`
}
type treeRoot struct {
	Name    string       `json:"name"`
	Entries []treeMember `json:"entries"`
}

func inventory(path string) (treeRoot, error) {
	out := treeRoot{Name: filepath.Base(path)}
	err := filepath.WalkDir(path, func(p string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, err := filepath.Rel(path, p)
		if err != nil {
			return err
		}
		e := treeMember{Path: filepath.ToSlash(rel), Directory: d.IsDir()}
		if !d.IsDir() {
			if !d.Type().IsRegular() {
				return fmt.Errorf("special_fixture_entry")
			}
			e.SHA, e.Size, err = fileSHA(p)
			if err != nil {
				return err
			}
		}
		out.Entries = append(out.Entries, e)
		return nil
	})
	sort.Slice(out.Entries, func(i, j int) bool { return out.Entries[i].Path < out.Entries[j].Path })
	return out, err
}
func (w *worker) prepareDirectory(v prepareRequest, id, dir string) (fixture, error) {
	f := fixtureData{Meta: fixture{ID: id, Kind: "directory"}}
	write := func(path string, size int64, seed string) error {
		if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
			return err
		}
		file, err := os.OpenFile(path, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
		if err != nil {
			return err
		}
		_, err = io.CopyN(file, &deterministicReader{seed: []byte(seed)}, size)
		ce := file.Close()
		if err == nil {
			err = ce
		}
		return err
	}
	root := filepath.Join(dir, "left", "目录 tree")
	if err := os.MkdirAll(filepath.Join(root, "nested", "empty"), 0700); err != nil {
		return fixture{}, err
	}
	f.Paths = []string{root}
	switch v.Payload.Size {
	case 0:
		for i, name := range []string{"nested/中文 空格.txt", ".hidden", "zero"} {
			size := int64(57)
			if i == 2 {
				size = 0
			}
			if err := write(filepath.Join(root, filepath.FromSlash(name)), size, v.Seed+name); err != nil {
				return fixture{}, err
			}
		}
		second := filepath.Join(dir, "right", "目录 tree")
		if err := os.MkdirAll(filepath.Join(second, "empty"), 0700); err != nil {
			return fixture{}, err
		}
		if err := write(filepath.Join(second, "member"), 83, v.Seed+"second"); err != nil {
			return fixture{}, err
		}
		single := filepath.Join(dir, "loose file.txt")
		if err := write(single, 29, v.Seed+"loose"); err != nil {
			return fixture{}, err
		}
		f.Paths = append(f.Paths, second, single)
	case 1024:
		for i := 0; i < 1024; i++ {
			name := fmt.Sprintf("nested/f-%04d", i)
			if err := write(filepath.Join(root, filepath.FromSlash(name)), 1024, v.Seed+name); err != nil {
				return fixture{}, err
			}
		}
	case 16 << 20:
		if err := write(filepath.Join(root, "large.bin"), 16<<20, v.Seed); err != nil {
			return fixture{}, err
		}
	default:
		return fixture{}, fmt.Errorf("unsupported_directory_recipe")
	}
	for _, path := range f.Paths {
		tree, err := inventory(path)
		if err != nil {
			return fixture{}, err
		}
		f.Meta.Trees = append(f.Meta.Trees, tree)
		for _, e := range tree.Entries {
			f.Meta.Total += e.Size
		}
	}
	w.fixtures[id] = f
	return f.Meta, nil
}
func (w *worker) verifyDirectory(ctx context.Context, r engine.ReceivedContent, dir string) (delivery, error) {
	// Export ran through the production CLI before this independent walk.
	actual := make([]treeRoot, 0, len(w.gate.Fixture.Trees))
	for i, tree := range w.gate.Fixture.Trees {
		container := filepath.Join(dir, fmt.Sprintf("r%d", i))
		entries, err := os.ReadDir(container)
		if err != nil || len(entries) != 1 || entries[0].Name() != tree.Name {
			return delivery{}, fmt.Errorf("directory_export_container_mismatch_%d", i)
		}
		got, err := inventory(filepath.Join(dir, fmt.Sprintf("r%d", i), tree.Name))
		if err != nil {
			return delivery{}, fmt.Errorf("directory_export_missing_root_%d", i)
		}
		if !reflect.DeepEqual(tree, got) {
			return delivery{}, fmt.Errorf("directory_export_inventory_mismatch_%d", i)
		}
		actual = append(actual, got)
	}
	containers, err := os.ReadDir(dir)
	if err != nil || len(containers) != len(actual) {
		return delivery{}, fmt.Errorf("directory_export_extra_roots")
	}
	return delivery{Trees: actual, Receipt: string(r.ReceiptID), History: string(r.HistoryID), ReceivedMS: r.ReceivedAtMs, VerifiedNS: time.Now().UnixNano(), Count: 1}, nil
}
