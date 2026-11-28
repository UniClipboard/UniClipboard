// Package fixture generates deterministic synthetic clipboard corpora and
// initializes isolated unic benchmark profiles without touching user state.
package fixture

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"image/png"
	"math/rand/v2"
	"os"
	"path/filepath"
	"strings"
)

// Preset identifies a deterministic synthetic corpus size.
type Preset string

const (
	PresetEmpty    Preset = "empty"
	PresetStandard Preset = "standard"
	PresetSoak     Preset = "soak"
)

// DefaultSeed is the fixed deterministic PRNG seed for corpus generation.
const DefaultSeed uint64 = 20261120

// Spec defines the exact counts and dimensions for a corpus preset.
type Spec struct {
	Preset      Preset `json:"preset"`
	Seed        uint64 `json:"seed"`
	TextCount   int    `json:"text_count"`
	ImageCount  int    `json:"image_count"`
	ImageWidth  int    `json:"image_width"`
	ImageHeight int    `json:"image_height"`
	FileCount   int    `json:"file_count"`
}

// ParsePreset validates and normalizes a corpus preset name.
func ParsePreset(raw string) (Preset, error) {
	switch Preset(strings.ToLower(strings.TrimSpace(raw))) {
	case PresetEmpty:
		return PresetEmpty, nil
	case PresetStandard:
		return PresetStandard, nil
	case PresetSoak:
		return PresetSoak, nil
	default:
		return "", fmt.Errorf("unknown preset %q (want empty, standard, or soak)", raw)
	}
}

// SpecForPreset returns the default specification for preset.
func SpecForPreset(preset Preset) (Spec, error) {
	switch preset {
	case PresetEmpty:
		return Spec{
			Preset: PresetEmpty,
			Seed:   DefaultSeed,
		}, nil
	case PresetStandard:
		return Spec{
			Preset:      PresetStandard,
			Seed:        DefaultSeed,
			TextCount:   200,
			ImageCount:  12,
			ImageWidth:  2048,
			ImageHeight: 1536,
			FileCount:   5,
		}, nil
	case PresetSoak:
		return Spec{
			Preset:      PresetSoak,
			Seed:        DefaultSeed,
			TextCount:   1000,
			ImageCount:  30,
			ImageWidth:  2048,
			ImageHeight: 1536,
			FileCount:   10,
		}, nil
	default:
		return Spec{}, fmt.Errorf("unsupported preset %q", preset)
	}
}

// TextItem describes one synthetic text clipboard entry.
type TextItem struct {
	Index    int    `json:"index"`
	Category string `json:"category"` // zh, en, code, link
	Content  string `json:"content"`
	Bytes    int    `json:"bytes"`
	SHA256   string `json:"sha256"`
}

// ImageItem describes one synthetic PNG image clipboard entry.
type ImageItem struct {
	Index        int    `json:"index"`
	RelativePath string `json:"relative_path"`
	Width        int    `json:"width"`
	Height       int    `json:"height"`
	Bytes        int64  `json:"bytes"`
	SHA256       string `json:"sha256"`
}

// FileItem describes one synthetic local file reference entry.
type FileItem struct {
	Index        int    `json:"index"`
	RelativePath string `json:"relative_path"`
	Name         string `json:"name"`
	Bytes        int64  `json:"bytes"`
	SHA256       string `json:"sha256"`
}

// Manifest records all deterministic assets written to a fixture directory.
type Manifest struct {
	SchemaVersion  int         `json:"schema_version"`
	Preset         Preset      `json:"preset"`
	Seed           uint64      `json:"seed"`
	TextCount      int         `json:"text_count"`
	ImageCount     int         `json:"image_count"`
	FileCount      int         `json:"file_count"`
	Texts          []TextItem  `json:"texts"`
	Images         []ImageItem `json:"images"`
	Files          []FileItem  `json:"files"`
	ManifestSHA256 string      `json:"manifest_sha256"`
}

// ExpectedCount returns the total number of clipboard history entries in the corpus.
func (m *Manifest) ExpectedCount() int {
	if m == nil {
		return 0
	}
	return len(m.Texts) + len(m.Images) + len(m.Files)
}

// Option customizes corpus generation.
type Option func(*Spec)

// WithSeed overrides the default PRNG seed.
func WithSeed(seed uint64) Option {
	return func(s *Spec) {
		s.Seed = seed
	}
}

// WithCounts overrides text, image, and file counts (useful for unit tests).
func WithCounts(texts, images, files int) Option {
	return func(s *Spec) {
		s.TextCount = texts
		s.ImageCount = images
		s.FileCount = files
	}
}

// WithImageDimensions overrides the generated PNG dimensions (useful for fast tests).
func WithImageDimensions(width, height int) Option {
	return func(s *Spec) {
		s.ImageWidth = width
		s.ImageHeight = height
	}
}

// Generate creates a deterministic synthetic corpus in dir and writes fixture-manifest.json.
func Generate(dir string, preset Preset, opts ...Option) (*Manifest, error) {
	if dir == "" {
		return nil, errors.New("fixture output directory is required")
	}
	spec, err := SpecForPreset(preset)
	if err != nil {
		return nil, err
	}
	for _, opt := range opts {
		if opt != nil {
			opt(&spec)
		}
	}
	if spec.TextCount < 0 || spec.ImageCount < 0 || spec.FileCount < 0 {
		return nil, errors.New("fixture counts must be non-negative")
	}
	if spec.ImageCount > 0 && (spec.ImageWidth <= 0 || spec.ImageHeight <= 0) {
		return nil, errors.New("image dimensions must be positive")
	}

	if err := os.MkdirAll(dir, 0o755); err != nil {
		return nil, fmt.Errorf("create fixture dir: %w", err)
	}
	if spec.ImageCount > 0 {
		if err := os.MkdirAll(filepath.Join(dir, "images"), 0o755); err != nil {
			return nil, fmt.Errorf("create images dir: %w", err)
		}
	}
	if spec.FileCount > 0 {
		if err := os.MkdirAll(filepath.Join(dir, "files"), 0o755); err != nil {
			return nil, fmt.Errorf("create files dir: %w", err)
		}
	}

	rng := rand.New(rand.NewPCG(spec.Seed, spec.Seed^0x9e3779b97f4a7c15))

	m := &Manifest{
		SchemaVersion: 1,
		Preset:        spec.Preset,
		Seed:          spec.Seed,
		TextCount:     spec.TextCount,
		ImageCount:    spec.ImageCount,
		FileCount:     spec.FileCount,
		Texts:         make([]TextItem, 0, spec.TextCount),
		Images:        make([]ImageItem, 0, spec.ImageCount),
		Files:         make([]FileItem, 0, spec.FileCount),
	}

	for i := 0; i < spec.TextCount; i++ {
		cat, content := syntheticText(i, rng)
		sum := sha256.Sum256([]byte(content))
		m.Texts = append(m.Texts, TextItem{
			Index:    i,
			Category: cat,
			Content:  content,
			Bytes:    len(content),
			SHA256:   hex.EncodeToString(sum[:]),
		})
	}

	enc := png.Encoder{CompressionLevel: png.BestSpeed}
	for i := 0; i < spec.ImageCount; i++ {
		rel := filepath.ToSlash(filepath.Join("images", fmt.Sprintf("image-%02d.png", i)))
		abs := filepath.Join(dir, filepath.FromSlash(rel))
		pngBytes, err := syntheticNoisePNG(spec.ImageWidth, spec.ImageHeight, rng, &enc)
		if err != nil {
			return nil, fmt.Errorf("encode image %d: %w", i, err)
		}
		if err := os.WriteFile(abs, pngBytes, 0o600); err != nil {
			return nil, fmt.Errorf("write image %d: %w", i, err)
		}
		sum := sha256.Sum256(pngBytes)
		m.Images = append(m.Images, ImageItem{
			Index:        i,
			RelativePath: rel,
			Width:        spec.ImageWidth,
			Height:       spec.ImageHeight,
			Bytes:        int64(len(pngBytes)),
			SHA256:       hex.EncodeToString(sum[:]),
		})
	}

	for i := 0; i < spec.FileCount; i++ {
		name, data := syntheticFile(i, rng)
		rel := filepath.ToSlash(filepath.Join("files", name))
		abs := filepath.Join(dir, filepath.FromSlash(rel))
		if err := os.WriteFile(abs, data, 0o600); err != nil {
			return nil, fmt.Errorf("write file %d: %w", i, err)
		}
		sum := sha256.Sum256(data)
		m.Files = append(m.Files, FileItem{
			Index:        i,
			RelativePath: rel,
			Name:         name,
			Bytes:        int64(len(data)),
			SHA256:       hex.EncodeToString(sum[:]),
		})
	}

	digest, err := computeManifestDigest(m)
	if err != nil {
		return nil, err
	}
	m.ManifestSHA256 = digest

	raw, err := json.MarshalIndent(m, "", "  ")
	if err != nil {
		return nil, fmt.Errorf("marshal fixture manifest: %w", err)
	}
	raw = append(raw, '\n')
	if err := os.WriteFile(filepath.Join(dir, "fixture-manifest.json"), raw, 0o600); err != nil {
		return nil, fmt.Errorf("write fixture-manifest.json: %w", err)
	}
	return m, nil
}

// LoadManifest reads and validates fixture-manifest.json from dir.
func LoadManifest(dir string) (*Manifest, error) {
	raw, err := os.ReadFile(filepath.Join(dir, "fixture-manifest.json"))
	if err != nil {
		return nil, fmt.Errorf("read fixture-manifest.json: %w", err)
	}
	var m Manifest
	if err := json.Unmarshal(raw, &m); err != nil {
		return nil, fmt.Errorf("parse fixture-manifest.json: %w", err)
	}
	if m.SchemaVersion != 1 {
		return nil, fmt.Errorf("unsupported fixture manifest schema version %d", m.SchemaVersion)
	}
	return &m, nil
}

// Verify checks that all assets in dir match the hashes recorded in m.
func (m *Manifest) Verify(dir string) error {
	if m == nil {
		return errors.New("nil manifest")
	}
	expectedDigest, err := computeManifestDigest(m)
	if err != nil {
		return err
	}
	if m.ManifestSHA256 != expectedDigest {
		return fmt.Errorf("manifest digest mismatch: got %s, want %s", m.ManifestSHA256, expectedDigest)
	}
	for _, t := range m.Texts {
		sum := sha256.Sum256([]byte(t.Content))
		if hex.EncodeToString(sum[:]) != t.SHA256 {
			return fmt.Errorf("text %d sha256 mismatch", t.Index)
		}
	}
	for _, img := range m.Images {
		data, err := os.ReadFile(filepath.Join(dir, filepath.FromSlash(img.RelativePath)))
		if err != nil {
			return fmt.Errorf("read image %s: %w", img.RelativePath, err)
		}
		sum := sha256.Sum256(data)
		if hex.EncodeToString(sum[:]) != img.SHA256 {
			return fmt.Errorf("image %s sha256 mismatch", img.RelativePath)
		}
	}
	for _, f := range m.Files {
		data, err := os.ReadFile(filepath.Join(dir, filepath.FromSlash(f.RelativePath)))
		if err != nil {
			return fmt.Errorf("read file %s: %w", f.RelativePath, err)
		}
		sum := sha256.Sum256(data)
		if hex.EncodeToString(sum[:]) != f.SHA256 {
			return fmt.Errorf("file %s sha256 mismatch", f.RelativePath)
		}
	}
	return nil
}

func computeManifestDigest(m *Manifest) (string, error) {
	clone := *m
	clone.ManifestSHA256 = ""
	raw, err := json.Marshal(clone)
	if err != nil {
		return "", err
	}
	sum := sha256.Sum256(raw)
	return hex.EncodeToString(sum[:]), nil
}

func syntheticText(i int, rng *rand.Rand) (string, string) {
	token := rng.Uint32()
	switch i % 4 {
	case 0:
		topics := []string{"剪贴板历史列表", "快捷面板预览", "本地数据库索引", "图片缩略图缓存", "窗口释放策略"}
		topic := topics[(i/4)%len(topics)]
		return "zh", fmt.Sprintf("合成中文语料 #%04d [%s]：用于验证桌面端列表渲染、搜索过滤与内存回收表现，随机校验码 %08x。支持多行换行展示与字形图集缓存测试。", i, topic, token)
	case 1:
		topics := []string{"clipboard-history", "quick-panel", "sqlite-query", "thumbnail-cache", "window-lifecycle"}
		topic := topics[(i/4)%len(topics)]
		return "en", fmt.Sprintf("Synthetic English note #%04d (%s): measuring steady-state private working set, physical footprint, and scroll rendering jitter under token %08x.", i, topic, token)
	case 2:
		return "code", fmt.Sprintf("package bench_%04d\n\nfunc SampleMetric(id int) uint64 {\n\treturn uint64(id)*0x%08x + 4096\n}", i, token)
	default:
		// RFC 2606 .invalid TLD prevents unintended outbound lookups by link previewers.
		return "link", fmt.Sprintf("https://bench.invalid/items/%04d?token=%08x&view=compact", i, token)
	}
}

func syntheticNoisePNG(w, h int, rng *rand.Rand, enc *png.Encoder) ([]byte, error) {
	img := image.NewNRGBA(image.Rect(0, 0, w, h))
	pix := img.Pix
	// Fill RGB with deterministic pseudo-random noise and opaque alpha so
	// encoders and GPU texture uploaders cannot optimize the image to a solid slab.
	var buf [8]byte
	for off := 0; off < len(pix); off += 4 {
		if (off & 4) == 0 {
			v := rng.Uint64()
			buf[0] = byte(v)
			buf[1] = byte(v >> 8)
			buf[2] = byte(v >> 16)
			buf[3] = byte(v >> 24)
			buf[4] = byte(v >> 32)
			buf[5] = byte(v >> 40)
			buf[6] = byte(v >> 48)
			buf[7] = byte(v >> 56)
			pix[off+0] = buf[0]
			pix[off+1] = buf[1]
			pix[off+2] = buf[2]
		} else {
			pix[off+0] = buf[3]
			pix[off+1] = buf[4]
			pix[off+2] = buf[5]
		}
		pix[off+3] = 255
	}
	var out bytes.Buffer
	out.Grow((w * h * 3) / 2)
	if err := enc.Encode(&out, img); err != nil {
		return nil, err
	}
	return out.Bytes(), nil
}

func syntheticFile(i int, rng *rand.Rand) (string, []byte) {
	exts := []string{"txt", "md", "json", "log", "csv"}
	ext := exts[i%len(exts)]
	name := fmt.Sprintf("sample-%02d.%s", i, ext)
	var b bytes.Buffer
	lineCount := 24 + (i % 16)
	for line := 0; line < lineCount; line++ {
		fmt.Fprintf(&b, "file=%s line=%03d payload=%016x\n", name, line, rng.Uint64())
	}
	return name, b.Bytes()
}
