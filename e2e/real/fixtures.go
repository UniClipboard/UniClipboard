package main

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"image"
	"image/png"
	"io"
	"os"
	"path/filepath"
)

type fixtureData struct {
	Meta  fixture
	Paths []string
}

// deterministicReader expands a seed in counter-sized SHA256 blocks. Preparing
// payloads is outside the measurement interval and uses no network content path.
type deterministicReader struct {
	seed    []byte
	counter uint64
	pending []byte
}

func (r *deterministicReader) Read(p []byte) (int, error) {
	n := 0
	for n < len(p) {
		if len(r.pending) == 0 {
			v := sha256.Sum256([]byte(fmt.Sprintf("%s/%d", r.seed, r.counter)))
			r.counter++
			r.pending = v[:]
		}
		k := copy(p[n:], r.pending)
		r.pending = r.pending[k:]
		n += k
	}
	return n, nil
}
func fileSHA(path string) (string, int64, error) {
	f, err := os.Open(path)
	if err != nil {
		return "", 0, err
	}
	defer f.Close()
	h := sha256.New()
	n, err := io.Copy(h, f)
	return hex.EncodeToString(h.Sum(nil)), n, err
}
func (w *worker) prepare(v prepareRequest) (fixture, error) {
	if len(v.Seed) == 0 || len(v.Seed) > 256 {
		return fixture{}, fmt.Errorf("invalid fixture seed")
	}
	id := digest([]byte(v.Seed + v.Payload.Kind + fmt.Sprint(v.Payload.Size)))[:24]
	if _, ok := w.fixtures[id]; ok {
		return fixture{}, fmt.Errorf("fixture already exists")
	}
	dir := filepath.Join(w.root, "fixtures", id)
	if err := os.MkdirAll(dir, 0700); err != nil {
		return fixture{}, err
	}
	f := fixtureData{Meta: fixture{ID: id, Kind: v.Payload.Kind}}
	add := func(kind, mime, name string, data []byte) error {
		path := filepath.Join(dir, name)
		if err := os.WriteFile(path, data, 0600); err != nil {
			return err
		}
		f.Paths = append(f.Paths, path)
		f.Meta.Parts = append(f.Meta.Parts, expectedPart{Kind: kind, MIME: mime, Name: name, Size: int64(len(data)), SHA: digest(data)})
		f.Meta.Total += int64(len(data))
		return nil
	}
	n := v.Payload.Size
	if n < 0 || n > 100<<20 {
		return fixture{}, fmt.Errorf("fixture size is invalid")
	}
	random := &deterministicReader{seed: []byte(v.Seed)}
	switch v.Payload.Kind {
	case "text", "html":
		if n < 1 || n > 64<<20 {
			return fixture{}, fmt.Errorf("empty or oversized representation")
		}
		b := make([]byte, n)
		if _, err := io.ReadFull(random, b); err != nil {
			return fixture{}, err
		}
		for i := range b {
			b[i] = "0123456789abcdef"[b[i]&15]
		}
		if err := add("representation", "text/plain", "plain.txt", b); err != nil {
			return fixture{}, err
		}
		if v.Payload.Kind == "html" {
			b = append([]byte(`<!doctype html><meta charset="utf-8"><p>`), b...)
			b = append(b, []byte("</p>")...)
			if err := add("representation", "text/html", "rich.html", b); err != nil {
				return fixture{}, err
			}
		}
	case "mail", "sensitive":
		f.Meta.Kind = "text"
		b := []byte("user-" + id + "@example.test")
		if v.Payload.Kind == "sensitive" {
			b = []byte("password: sentinel-" + id)
			w.secrets = append(w.secrets, b)
		}
		if err := add("representation", "text/plain", "plain.txt", b); err != nil {
			return fixture{}, err
		}
	case "png":
		if n < 1 || n > 2048 {
			return fixture{}, fmt.Errorf("PNG dimensions are invalid")
		}
		im := image.NewNRGBA(image.Rect(0, 0, int(n), int(n)))
		if _, err := io.ReadFull(random, im.Pix); err != nil {
			return fixture{}, err
		}
		for i := 3; i < len(im.Pix); i += 4 {
			im.Pix[i] = 255
		}
		path := filepath.Join(dir, "image.png")
		out, err := os.Create(path)
		if err != nil {
			return fixture{}, err
		}
		err = png.Encode(out, im)
		closeErr := out.Close()
		if err != nil {
			return fixture{}, err
		}
		if closeErr != nil {
			return fixture{}, closeErr
		}
		sha, size, err := fileSHA(path)
		if err != nil {
			return fixture{}, err
		}
		f.Paths = []string{path}
		f.Meta.Parts = []expectedPart{{Kind: "representation", MIME: "image/png", Name: "image.png", Size: size, SHA: sha}}
		f.Meta.Total = size
	case "file":
		name := "payload-" + id + ".bin"
		path := filepath.Join(dir, name)
		out, err := os.Create(path)
		if err != nil {
			return fixture{}, err
		}
		_, err = io.CopyN(out, random, n)
		closeErr := out.Close()
		if err != nil {
			return fixture{}, err
		}
		if closeErr != nil {
			return fixture{}, closeErr
		}
		sha, size, err := fileSHA(path)
		if err != nil {
			return fixture{}, err
		}
		f.Paths = []string{path}
		f.Meta.Parts = []expectedPart{{Kind: "file", MIME: "application/octet-stream", Name: name, Size: size, SHA: sha}}
		f.Meta.Total = size
	default:
		return fixture{}, fmt.Errorf("unsupported fixture kind")
	}
	w.fixtures[id] = f
	return f.Meta, nil
}
func digest(b []byte) string { h := sha256.Sum256(b); return hex.EncodeToString(h[:]) }
