package update

import (
	"errors"
	"io"
	"os"
	"sync"
)

// Payload owns one private, verified update file. WithFile pins it while an
// installer reads it; Close waits for that reader, then removes all staging.
type Payload struct {
	mu   sync.Mutex
	file *os.File
	dir  string
	Size int64
}

func (p *Payload) WithFile(use func(*os.File) error) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.file == nil {
		return os.ErrClosed
	}
	if _, err := p.file.Seek(0, io.SeekStart); err != nil {
		return err
	}
	return use(p.file)
}

func (p *Payload) Close() error {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.file == nil {
		return nil
	}
	err := p.file.Close()
	p.file = nil
	return errors.Join(err, os.RemoveAll(p.dir))
}
