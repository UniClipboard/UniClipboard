// Command package builds reusable headless binaries and optional desktop archives.
package main

import (
	"archive/tar"
	"archive/zip"
	"compress/gzip"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"time"
)

func main() {
	if e := run(); e != nil {
		fmt.Fprintln(os.Stderr, e)
		os.Exit(1)
	}
}
func run() error {
	desktop := flag.Bool("desktop", false, "also build the desktop package")
	output := flag.String("output", "dist/releases", "output directory")
	flag.Parse()
	targetOS := os.Getenv("GOOS")
	if targetOS == "" {
		targetOS = runtime.GOOS
	}
	arch := os.Getenv("GOARCH")
	if arch == "" {
		arch = runtime.GOARCH
	}
	suffix := ""
	if targetOS == "windows" {
		suffix = ".exe"
	}
	if e := os.MkdirAll(*output, 0755); e != nil {
		return e
	}
	work, e := os.MkdirTemp("", "unic-package")
	if e != nil {
		return e
	}
	defer func() { _ = os.RemoveAll(work) }()
	names := []string{"unic", "unicd"}
	if *desktop {
		names = append(names, "unic-desktop")
	}
	source, err := sourceHashes(names)
	if err != nil {
		return err
	}
	archives := map[string]string{}
	hashes := map[string]string{}
	for _, name := range names {
		app := "service"
		if name == "unic-desktop" {
			app = "desktop"
		}
		path := filepath.Join(work, name+suffix)
		cmd := exec.Command("go", "build", "-trimpath", "-buildvcs=false", "-o", path, "./app/"+app+"/cmd/"+name)
		cmd.Stdout, cmd.Stderr = os.Stdout, os.Stderr
		if e = cmd.Run(); e != nil {
			return e
		}
		f, e := os.Open(path)
		if e != nil {
			return e
		}
		h := sha256.New()
		_, e = io.Copy(h, f)
		_ = f.Close()
		if e != nil {
			return e
		}
		hashes[name+suffix] = hex.EncodeToString(h.Sum(nil))
	}
	for _, kind := range []string{"headless", "desktop"} {
		if kind == "desktop" && !*desktop {
			continue
		}
		files := []string{"unic" + suffix, "unicd" + suffix}
		if kind == "desktop" {
			files = append(files, "unic-desktop"+suffix)
		}
		base := "unic-" + kind + "-" + targetOS + "-" + arch
		ext := ".tar.gz"
		if targetOS == "windows" {
			ext = ".zip"
		}
		path := filepath.Join(*output, base+ext)
		if e = archive(path, work, files, targetOS == "windows"); e != nil {
			return e
		}
		b, e := os.ReadFile(path)
		if e != nil {
			return e
		}
		sum := sha256.Sum256(b)
		archives[filepath.Base(path)] = hex.EncodeToString(sum[:])
		fmt.Println(path)
	}
	head, _ := exec.Command("git", "rev-parse", "HEAD").Output()
	dirty, _ := exec.Command("git", "status", "--porcelain").Output()
	after, e := sourceHashes(names)
	if e != nil {
		return e
	}
	if !reflect.DeepEqual(source, after) {
		return fmt.Errorf("source changed during packaging")
	}
	manifest := map[string]any{"source_sha256": source, "source_unchanged": true, "archives_sha256": archives, "os": targetOS, "arch": arch, "go": runtime.Version(), "source_head": strings.TrimSpace(string(head)), "source_dirty": len(dirty) > 0, "binaries_sha256": hashes, "desktop": *desktop, "signed": false}
	data, e := json.MarshalIndent(manifest, "", "  ")
	if e != nil {
		return e
	}
	return os.WriteFile(filepath.Join(*output, "manifest-"+targetOS+"-"+arch+".json"), data, 0644)
}
func archive(dest, dir string, names []string, asZip bool) (err error) {
	f, err := os.Create(dest)
	if err != nil {
		return err
	}
	defer func() {
		if e := f.Close(); err == nil {
			err = e
		}
		if err != nil {
			_ = os.Remove(dest)
		}
	}()
	if asZip {
		z := zip.NewWriter(f)
		for _, name := range names {
			in, e := os.Open(filepath.Join(dir, name))
			if e != nil {
				return e
			}
			info, e := in.Stat()
			if e != nil {
				_ = in.Close()
				return e
			}
			h, e := zip.FileInfoHeader(info)
			if e != nil {
				_ = in.Close()
				return e
			}
			h.Name = name
			h.Modified = time.Date(1980, 1, 1, 0, 0, 0, 0, time.UTC)
			w, e := z.CreateHeader(h)
			if e == nil {
				_, e = io.Copy(w, in)
			}
			_ = in.Close()
			if e != nil {
				return e
			}
		}
		return z.Close()
	}
	gz := gzip.NewWriter(f)
	tw := tar.NewWriter(gz)
	for _, name := range names {
		in, e := os.Open(filepath.Join(dir, name))
		if e != nil {
			return e
		}
		info, e := in.Stat()
		if e != nil {
			_ = in.Close()
			return e
		}
		h, e := tar.FileInfoHeader(info, "")
		if e != nil {
			_ = in.Close()
			return e
		}
		h.Name = name
		h.ModTime = time.Unix(0, 0)
		h.Uid = 0
		h.Gid = 0
		h.Uname = ""
		h.Gname = ""
		if e = tw.WriteHeader(h); e == nil {
			_, e = io.Copy(tw, in)
		}
		_ = in.Close()
		if e != nil {
			return e
		}
	}
	if err = tw.Close(); err != nil {
		return err
	}
	return gz.Close()
}

func sourceHashes(names []string) (map[string]string, error) {
	root, err := os.Getwd()
	if err != nil {
		return nil, err
	}
	args := []string{"list", "-deps", "-json"}
	for _, name := range names {
		app := "service"
		if name == "unic-desktop" {
			app = "desktop"
		}
		args = append(args, "./app/"+app+"/cmd/"+name)
	}
	cmd := exec.Command("go", args...)
	data, err := cmd.Output()
	if err != nil {
		return nil, err
	}
	decoder := json.NewDecoder(strings.NewReader(string(data)))
	paths := []string{"go.mod", "go.sum"}
	for {
		var pkg struct {
			Dir                                                                        string
			GoFiles, CgoFiles, CFiles, CXXFiles, HFiles, SFiles, SysoFiles, EmbedFiles []string
		}
		err = decoder.Decode(&pkg)
		if err == io.EOF {
			break
		}
		if err != nil {
			return nil, err
		}
		rel, err := filepath.Rel(root, pkg.Dir)
		if err != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(os.PathSeparator)) {
			continue
		}
		for _, group := range [][]string{pkg.GoFiles, pkg.CgoFiles, pkg.CFiles, pkg.CXXFiles, pkg.HFiles, pkg.SFiles, pkg.SysoFiles, pkg.EmbedFiles} {
			for _, name := range group {
				paths = append(paths, filepath.Join(rel, name))
			}
		}
	}
	hashes := map[string]string{}
	for _, path := range paths {
		data, err := os.ReadFile(path)
		if err != nil {
			return nil, err
		}
		sum := sha256.Sum256(data)
		hashes[filepath.ToSlash(path)] = hex.EncodeToString(sum[:])
	}
	return hashes, nil
}
