// Package hostlog writes the Go processes' (GUI host, CLI) persistent log: one JSON object per line in
// <log dir>/<stem>.<YYYY-MM-DD>, the same shape and retention as the Rust host files, so the diagnostic export
// treats every role alike.
package hostlog

import (
	"bytes"
	"crypto/rand"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
)

const (
	// MaxMessageBytes bounds one record's message.
	MaxMessageBytes = 8192
	// retainDays matches the Rust host files: older daily files are removed at start.
	retainDays = 7
	target     = "go.host"
)

// Options configures a Logger. Now and Stderr exist for tests.
type Options struct {
	Dir    string
	Stem   string // file stem, for example uniclipboard-gui
	Now    func() time.Time
	Stderr io.Writer // echo of every record's message; os.Stderr when nil
}

// Logger is the process's persistent log.
type Logger struct {
	dir, stem string
	now       func() time.Time
	stderr    io.Writer
	runID     string
	pid       int

	mu   sync.Mutex
	day  string
	file *os.File
}

// Open creates the log directory, removes this role's expired files and returns a Logger. A failure leaves the
// caller with its console output only; it is returned, never fatal.
func Open(options Options) (*Logger, error) {
	logger := &Logger{dir: options.Dir, stem: options.Stem, now: options.Now, stderr: options.Stderr, runID: newRunID(), pid: os.Getpid()}
	if logger.now == nil {
		logger.now = time.Now
	}
	if logger.stderr == nil {
		logger.stderr = os.Stderr
	}
	if err := os.MkdirAll(logger.dir, 0o700); err != nil {
		return nil, err
	}
	logger.prune()
	if err := logger.rotate(logger.now().UTC().Format("2006-01-02")); err != nil {
		return nil, err
	}
	return logger, nil
}

// OpenDefault opens the role's log in the platform log directory. It returns nil when the directory cannot be
// resolved or opened; the nil Logger is safe to use and the process keeps its console output.
func OpenDefault(stem string) *Logger {
	dir, ok := apppaths.AppLogDir()
	if !ok {
		return nil
	}
	logger, err := Open(Options{Dir: dir, Stem: stem})
	if err != nil {
		fmt.Fprintf(os.Stderr, "hostlog: persistent log unavailable: %v\n", err)
		return nil
	}
	return logger
}

func newRunID() string {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		return "00000000-0000-4000-8000-000000000000"
	}
	b[6] = b[6]&0x0f | 0x40
	b[8] = b[8]&0x3f | 0x80
	return fmt.Sprintf("%x-%x-%x-%x-%x", b[0:4], b[4:6], b[6:8], b[8:10], b[10:16])
}

// prune removes this role's daily files older than the retention window; other roles' files are not touched.
func (l *Logger) prune() {
	entries, err := os.ReadDir(l.dir)
	if err != nil {
		return
	}
	cutoff := l.now().UTC().AddDate(0, 0, -retainDays).Format("2006-01-02")
	prefix := l.stem + ".json."
	for _, entry := range entries {
		date, ok := strings.CutPrefix(entry.Name(), prefix)
		if !ok || len(date) != 10 {
			continue
		}
		if _, err := time.Parse("2006-01-02", date); err == nil && date < cutoff {
			_ = os.Remove(filepath.Join(l.dir, entry.Name()))
		}
	}
}

func (l *Logger) rotate(day string) error {
	file, err := os.OpenFile(filepath.Join(l.dir, l.stem+".json."+day), os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o600)
	if err != nil {
		return err
	}
	if l.file != nil {
		_ = l.file.Close()
	}
	l.file, l.day = file, day
	return nil
}

// Info, Warn and Error write one record.
func (l *Logger) Info(format string, args ...any)  { l.write("INFO", fmt.Sprintf(format, args...)) }
func (l *Logger) Warn(format string, args ...any)  { l.write("WARN", fmt.Sprintf(format, args...)) }
func (l *Logger) Error(format string, args ...any) { l.write("ERROR", fmt.Sprintf(format, args...)) }

func (l *Logger) write(level, message string) {
	if l == nil {
		return
	}
	if len(message) > MaxMessageBytes {
		message = strings.ToValidUTF8(message[:MaxMessageBytes], "") + "…[truncated]"
	}
	now := l.now().UTC()
	line, err := json.Marshal(map[string]any{
		"timestamp": now.Format("2006-01-02T15:04:05.000Z"),
		"level":     level,
		"target":    target,
		"message":   message,
		"run_id":    l.runID,
		"pid":       l.pid,
	})
	if err != nil {
		return
	}
	line = append(line, '\n')

	l.mu.Lock()
	if l.file == nil { // closed: late records are dropped silently
		l.mu.Unlock()
		return
	}
	if day := now.Format("2006-01-02"); day != l.day {
		// A failed rotation keeps appending to the previous day's file rather than dropping the record.
		_ = l.rotate(day)
	}
	_, werr := l.file.Write(line)
	l.mu.Unlock()
	if werr != nil {
		fmt.Fprintf(l.stderr, "hostlog: write failed: %v\n", werr)
	}
}

// RedirectStandardLog sends the standard library logger (log.Print and friends) to this log at INFO and still
// echoes each line to stderr, so existing call sites keep working unchanged.
func (l *Logger) RedirectStandardLog() {
	if l == nil {
		return
	}
	// The record carries its own timestamp, so the standard prefix is dropped and re-added to the echo only.
	log.SetFlags(0)
	log.SetOutput(standardLogWriter{l})
}

type standardLogWriter struct{ l *Logger }

func (w standardLogWriter) Write(p []byte) (int, error) {
	_, _ = io.WriteString(w.l.stderr, w.l.now().Format("2006/01/02 15:04:05 "))
	_, _ = w.l.stderr.Write(p)
	w.l.write("INFO", string(bytes.TrimRight(p, "\n")))
	return len(p), nil
}

// Close releases the file.
func (l *Logger) Close() error {
	if l == nil {
		return nil
	}
	l.mu.Lock()
	defer l.mu.Unlock()
	if l.file == nil {
		return nil
	}
	err := l.file.Close()
	l.file = nil
	return err
}
