package main

import (
	"archive/tar"
	"compress/gzip"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"

	paths "github.com/uniclipboard/unic/internal/localpaths"
)

const logExportUsage = "usage: unic dev logs export [--output FILE]"
const maxLogSegmentBytes int64 = 4 << 20
const maxLogExportBytes int64 = 20 << 20

type selectedLog struct {
	name     string
	info     os.FileInfo
	sequence int64
}
type exportedLog struct {
	Name   string `json:"name"`
	Bytes  int64  `json:"bytes"`
	SHA256 string `json:"sha256"`
}
type logArchiveManifest struct {
	FormatVersion int           `json:"format_version"`
	CreatedAt     time.Time     `json:"created_at"`
	Files         []exportedLog `json:"files"`
}

func runLogExport(ctx context.Context, args []string) error {
	flags := flag.NewFlagSet("logs export", flag.ContinueOnError)
	flags.SetOutput(io.Discard)
	output := flags.String("output", "unic-logs-"+time.Now().UTC().Format("20060102T150405.000000000Z")+".tar.gz", "")
	if err := flags.Parse(args); errors.Is(err, flag.ErrHelp) {
		fmt.Println(logExportUsage)
		return nil
	} else if err != nil {
		return errors.New(logExportUsage)
	}
	if flags.NArg() != 0 || *output == "" {
		return errors.New(logExportUsage)
	}
	profile, err := paths.Dir()
	if err != nil {
		return errors.New("cannot open the local profile")
	}
	archive, err := exportLocalLogs(ctx, profile, *output)
	if err != nil {
		return err
	}
	fmt.Println(archive)
	return nil
}

// exportLocalLogs uses private rooted handles and exclusive creation. It copies
// only retained on-disk segments; it does not contact or flush the service.
func exportLocalLogs(ctx context.Context, profile, output string) (string, error) {
	base, err := os.OpenRoot(profile)
	if err != nil {
		return "", errors.New("cannot open the local profile")
	}
	defer base.Close()
	info, err := base.Lstat("logs")
	if err != nil {
		return "", errors.New("local logs are unavailable")
	}
	if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return "", errors.New("local logs must be a regular directory")
	}
	logs, err := base.OpenRoot("logs")
	if err != nil {
		return "", errors.New("cannot open local logs")
	}
	defer logs.Close()
	actual, err := logs.Stat(".")
	if err != nil || !os.SameFile(info, actual) {
		return "", errors.New("local logs directory changed; retry export")
	}
	selected, err := selectLocalLogs(logs)
	if err != nil {
		return "", err
	}
	absolute, err := filepath.Abs(output)
	if err != nil {
		return "", errors.New("invalid archive destination")
	}
	destination, err := os.OpenRoot(filepath.Dir(absolute))
	if err != nil {
		return "", errors.New("archive destination directory is unavailable")
	}
	defer destination.Close()
	parent, err := destination.Stat(".")
	if err != nil {
		return "", errors.New("cannot inspect archive destination")
	}
	if os.SameFile(actual, parent) {
		return "", errors.New("archive destination must be outside the managed logs directory")
	}
	name := filepath.Base(absolute)
	file, err := destination.OpenFile(name, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if errors.Is(err, fs.ErrExist) {
		return "", errors.New("archive destination already exists")
	}
	if err != nil {
		return "", errors.New("cannot create log archive")
	}
	owned, statErr := file.Stat()
	if statErr != nil {
		_ = file.Close()
		_ = destination.Remove(name)
		return "", errors.New("cannot inspect new log archive")
	}
	complete := false
	defer func() {
		_ = file.Close()
		if !complete {
			if current, err := destination.Lstat(name); err == nil && os.SameFile(owned, current) {
				_ = destination.Remove(name)
			}
		}
	}()
	if err := writeLogArchive(ctx, file, logs, selected); err != nil {
		return "", err
	}
	if err := file.Sync(); err != nil {
		return "", errors.New("cannot save log archive")
	}
	if err := file.Close(); err != nil {
		return "", errors.New("cannot close log archive")
	}
	complete = true
	return absolute, nil
}

func selectLocalLogs(root *os.Root) ([]selectedLog, error) {
	entries, err := fs.ReadDir(root.FS(), ".")
	if err != nil {
		return nil, errors.New("cannot list local logs")
	}
	var selected []selectedLog
	var total int64
	for _, entry := range entries {
		name := entry.Name()
		if !strings.HasPrefix(name, "segment-") || !strings.HasSuffix(name, ".jsonl") {
			continue
		}
		sequence, err := strconv.ParseInt(strings.TrimSuffix(strings.TrimPrefix(name, "segment-"), ".jsonl"), 10, 64)
		if err != nil || sequence <= 0 {
			continue
		}
		info, err := root.Lstat(name)
		if os.IsNotExist(err) {
			continue
		} // retention removed an unopened segment
		if err != nil {
			return nil, errors.New("cannot inspect local log segment")
		}
		if !info.Mode().IsRegular() {
			return nil, errors.New("local log segments must be regular files")
		}
		size := info.Size()
		if size < 0 || size > maxLogSegmentBytes || size > maxLogExportBytes-total {
			return nil, errors.New("local logs exceed archive size limits")
		}
		total += size
		selected = append(selected, selectedLog{name: name, info: info, sequence: sequence})
	}
	if len(selected) == 0 {
		return nil, errors.New("no local log segments to export")
	}
	sort.Slice(selected, func(i, j int) bool {
		if selected[i].sequence == selected[j].sequence {
			return selected[i].name < selected[j].name
		}
		return selected[i].sequence < selected[j].sequence
	})
	return selected, nil
}

func writeLogArchive(ctx context.Context, out io.Writer, root *os.Root, selected []selectedLog) error {
	gz := gzip.NewWriter(out)
	defer gz.Close()
	archive := tar.NewWriter(gz)
	defer archive.Close()
	manifest := logArchiveManifest{FormatVersion: 1, CreatedAt: time.Now().UTC()}
	for _, segment := range selected {
		if err := ctx.Err(); err != nil {
			return errors.New("log export cancelled")
		}
		record, err := copyLogSegment(ctx, archive, root, segment)
		if err != nil {
			return err
		}
		if record != nil {
			manifest.Files = append(manifest.Files, *record)
		}
	}
	if len(manifest.Files) == 0 {
		return errors.New("local log segments were removed; retry export")
	}
	data, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return errors.New("cannot encode log archive manifest")
	}
	data = append(data, '\n')
	if err := archive.WriteHeader(&tar.Header{Name: "manifest.json", Mode: 0600, Typeflag: tar.TypeReg, Size: int64(len(data))}); err != nil {
		return errors.New("cannot write log archive manifest")
	}
	if _, err := archive.Write(data); err != nil {
		return errors.New("cannot write log archive manifest")
	}
	if err := archive.Close(); err != nil {
		return errors.New("cannot finish log archive")
	}
	if err := gz.Close(); err != nil {
		return errors.New("cannot finish compressed log archive")
	}
	return nil
}

func copyLogSegment(ctx context.Context, archive *tar.Writer, root *os.Root, segment selectedLog) (*exportedLog, error) {
	file, err := root.Open(segment.name)
	if os.IsNotExist(err) {
		return nil, nil
	}
	if err != nil {
		return nil, errors.New("cannot open local log segment")
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil || !info.Mode().IsRegular() || !os.SameFile(segment.info, info) {
		return nil, errors.New("local log segment changed; retry export")
	}
	if info.Size() < segment.info.Size() {
		return nil, errors.New("local log segment was truncated; retry export")
	}
	name := "logs/" + segment.name
	size := segment.info.Size()
	if err := archive.WriteHeader(&tar.Header{Name: name, Mode: 0600, Typeflag: tar.TypeReg, Size: size, ModTime: segment.info.ModTime().UTC().Truncate(time.Second)}); err != nil {
		return nil, errors.New("cannot write log archive")
	}
	hash := sha256.New()
	if _, err := io.CopyN(io.MultiWriter(archive, hash), logSnapshotReader{ctx: ctx, reader: file}, size); err != nil {
		return nil, errors.New("cannot copy local log segment")
	}
	if err := file.Close(); err != nil {
		return nil, errors.New("cannot close local log segment")
	}
	return &exportedLog{Name: name, Bytes: size, SHA256: hex.EncodeToString(hash.Sum(nil))}, nil
}

type logSnapshotReader struct {
	ctx    context.Context
	reader io.Reader
}

func (r logSnapshotReader) Read(p []byte) (int, error) {
	if err := r.ctx.Err(); err != nil {
		return 0, err
	}
	return r.reader.Read(p)
}
