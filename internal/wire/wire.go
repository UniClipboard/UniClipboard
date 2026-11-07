// Package wire is the framing used between two unic daemons over a tailcat
// tunnel. A control message is a length-prefixed JSON document; a clipboard
// message is followed by the raw bytes of each part, whose sizes the
// control message announces.
package wire

import (
	"bufio"
	"crypto/rand"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"unic/internal/clip"
)

const (
	Version      = 2
	MaxControl   = 1 << 20  // largest JSON control message
	MaxPartBytes = 64 << 20 // in-memory format (text, HTML, PNG), same cap as mygo
	MaxFileBytes = 2 << 30  // one file
	MaxTotal     = 4 << 30  // whole clipboard message
	MaxParts     = 4096
)

// Hello opens every connection, in both directions.
type Hello struct {
	T       string `json:"t"` // "hello"
	Version int    `json:"v"`
	ID      string `json:"id"`   // stable device id
	Name    string `json:"name"` // device name
	Addr    string `json:"addr"` // the sender's own tailcat address, so the peer can dial back
	// Pair is the token of the pairing code the dialer used to find this
	// device, if any. It tells the receiver that the request came through the
	// code it is showing; it grants no trust by itself.
	Pair string `json:"pair,omitempty"`
}

// Verdict follows the hellos: whether the sender accepts the other device.
// A device accepts a peer it has paired with, or one it is pairing with right
// now; anyone else has to be approved by the user first.
type Verdict struct {
	T  string `json:"t"` // "verdict"
	OK bool   `json:"ok"`
}

// PeerInfo describes a paired device, for introducing it to another one.
type PeerInfo struct {
	ID   string `json:"id"`
	Name string `json:"name"`
	Addr string `json:"addr"`
}

// Peers introduces the sender's paired devices. The receiver treats them as
// candidates that its user must approve; nothing connects by itself.
type Peers struct {
	T     string     `json:"t"` // "peers"
	Peers []PeerInfo `json:"peers"`
}

// MaxIntroduced bounds the candidates one message may add.
const MaxIntroduced = 64

// PartMeta announces one blob that follows a Clip message.
type PartMeta struct {
	Kind string `json:"kind"` // "fmt" or "file"
	Name string `json:"name"` // format (MIME) or file name
	Size int64  `json:"size"`
}

// Clip announces a clipboard change made on Origin.
type Clip struct {
	T      string     `json:"t"` // "clip"
	ID     string     `json:"id"`
	Origin string     `json:"origin"`
	TS     int64      `json:"ts"` // unix milliseconds on the origin
	Hash   string     `json:"hash"`
	Parts  []PartMeta `json:"parts"`
}

// Conn wraps a stream with message framing. Reads and writes may happen
// concurrently with each other, but not two writes or two reads.
type Conn struct {
	r *bufio.Reader
	w io.Writer
}

func NewConn(rw io.ReadWriter) *Conn { return &Conn{r: bufio.NewReaderSize(rw, 64<<10), w: rw} }

func (c *Conn) WriteJSON(v any) error {
	b, err := json.Marshal(v)
	if err != nil {
		return err
	}
	if len(b) > MaxControl {
		return errors.New("wire: control message too large")
	}
	var hdr [4]byte
	binary.BigEndian.PutUint32(hdr[:], uint32(len(b)))
	_, err = c.w.Write(append(hdr[:], b...))
	return err
}

// ReadRaw returns the next control message and its type.
func (c *Conn) ReadRaw() (typ string, raw []byte, err error) {
	var hdr [4]byte
	if _, err = io.ReadFull(c.r, hdr[:]); err != nil {
		return "", nil, err
	}
	n := binary.BigEndian.Uint32(hdr[:])
	if n > MaxControl {
		return "", nil, fmt.Errorf("wire: control message of %d bytes", n)
	}
	raw = make([]byte, n)
	if _, err = io.ReadFull(c.r, raw); err != nil {
		return "", nil, err
	}
	var t struct {
		T string `json:"t"`
	}
	if err = json.Unmarshal(raw, &t); err != nil {
		return "", nil, err
	}
	return t.T, raw, nil
}

// NewID returns a random hex id.
func NewID() string {
	var b [16]byte
	_, _ = rand.Read(b[:])
	return hex.EncodeToString(b[:])
}

// SendClip writes a Clip message followed by the bytes of its parts.
func (c *Conn) SendClip(id, origin string, ts int64, s clip.Snapshot) error {
	m := Clip{T: "clip", ID: id, Origin: origin, TS: ts, Hash: s.Hash()}
	for _, p := range s.Parts {
		m.Parts = append(m.Parts, PartMeta{Kind: "fmt", Name: p.Format, Size: int64(len(p.Data))})
	}
	for _, f := range s.Files {
		m.Parts = append(m.Parts, PartMeta{Kind: "file", Name: f.Name, Size: f.Size})
	}
	if err := c.WriteJSON(m); err != nil {
		return err
	}
	for _, p := range s.Parts {
		if _, err := c.w.Write(p.Data); err != nil {
			return err
		}
	}
	for _, f := range s.Files {
		if err := c.copyFile(f); err != nil {
			return err
		}
	}
	return nil
}

// OutPart is one blob of a clip sent from a stream.
type OutPart struct {
	Kind string // "fmt" or "file"
	Name string // format (MIME) or file name
	Size int64
	R    io.Reader // yields exactly Size bytes
}

// SendStream writes a Clip message followed by the bytes of parts. hash must
// be what the receiver computes from the received content (see clip.Snapshot
// .Hash). A short reader is an error; the stream is then out of step and the
// connection must be dropped.
func (c *Conn) SendStream(id, origin string, ts int64, hash string, parts []OutPart) error {
	m := Clip{T: "clip", ID: id, Origin: origin, TS: ts, Hash: hash}
	for _, p := range parts {
		m.Parts = append(m.Parts, PartMeta{Kind: p.Kind, Name: p.Name, Size: p.Size})
	}
	if err := c.WriteJSON(m); err != nil {
		return err
	}
	for _, p := range parts {
		if _, err := io.CopyN(c.w, p.R, p.Size); err != nil {
			return err
		}
	}
	return nil
}

func (c *Conn) copyFile(f clip.File) error {
	src, err := os.Open(f.Path)
	if err != nil {
		return err
	}
	defer func() { _ = src.Close() }()
	// The announced size is a promise; a file that changed underneath us
	// would desynchronize the stream, so refuse rather than guess.
	st, err := src.Stat()
	if err != nil {
		return err
	}
	if st.Size() != f.Size {
		return fmt.Errorf("wire: %s changed size while sending", f.Name)
	}
	_, err = io.CopyN(c.w, src, f.Size)
	return err
}

// Validate checks a Clip message against the size limits before any blob is read.
func (m *Clip) Validate() error {
	if len(m.Parts) > MaxParts {
		return errors.New("wire: too many parts")
	}
	var total int64
	for _, p := range m.Parts {
		if p.Size < 0 {
			return errors.New("wire: negative part size")
		}
		switch p.Kind {
		case "fmt":
			if p.Size > MaxPartBytes {
				return fmt.Errorf("wire: %s is %d bytes, over the limit", p.Name, p.Size)
			}
		case "file":
			if p.Size > MaxFileBytes {
				return fmt.Errorf("wire: file %s is %d bytes, over the limit", p.Name, p.Size)
			}
		default:
			return fmt.Errorf("wire: unknown part kind %q", p.Kind)
		}
		total += p.Size
	}
	if total > MaxTotal {
		return errors.New("wire: clipboard message too large")
	}
	return nil
}

// Limits are the size limits a device applies to the clips it sends and
// receives. They may be tighter than the protocol's hard caps above.
type Limits struct {
	File  int64 // one file
	Total int64 // whole clipboard message
}

// HardLimits are the protocol's caps.
var HardLimits = Limits{File: MaxFileBytes, Total: MaxTotal}

// Fits reports whether m is within l. m must have passed Validate.
func (m *Clip) Fits(l Limits) error {
	var total int64
	for _, p := range m.Parts {
		if p.Kind == "file" && p.Size > l.File {
			return fmt.Errorf("file %s is %d bytes, over the configured limit", p.Name, p.Size)
		}
		total += p.Size
	}
	if total > l.Total {
		return errors.New("clipboard is over the configured size limit")
	}
	return nil
}

// Discard reads and drops the blobs announced by m, keeping the stream in
// sync when the clip is not wanted. m must have passed Validate.
func (c *Conn) Discard(m *Clip) error {
	var total int64
	for _, p := range m.Parts {
		total += p.Size
	}
	_, err := io.CopyN(io.Discard, c.r, total)
	return err
}

// ReceiveClip reads the blobs announced by m. Files are written into a new
// directory below inbox, which the caller owns and removes when done.
// On error the stream is no longer usable.
func (c *Conn) ReceiveClip(m *Clip, inbox string) (s clip.Snapshot, dir string, err error) {
	if err = m.Validate(); err != nil {
		return
	}
	defer func() {
		if err != nil && dir != "" {
			_ = os.RemoveAll(dir)
			dir = ""
		}
	}()
	for _, p := range m.Parts {
		switch p.Kind {
		case "fmt":
			buf := make([]byte, p.Size)
			if _, err = io.ReadFull(c.r, buf); err != nil {
				return
			}
			s.Parts = append(s.Parts, clip.Part{Format: p.Name, Data: buf})
		case "file":
			if dir == "" {
				if err = os.MkdirAll(inbox, 0o700); err != nil {
					return
				}
				if dir, err = os.MkdirTemp(inbox, "clip-"); err != nil {
					return
				}
			}
			name := SafeName(p.Name)
			// Each file gets its own directory so equal names cannot collide
			// and the original name is kept for the receiving application.
			sub, merr := os.MkdirTemp(dir, "f-")
			if merr != nil {
				err = merr
				return
			}
			path := filepath.Join(sub, name)
			if err = writeN(path, c.r, p.Size); err != nil {
				return
			}
			s.Files = append(s.Files, clip.File{Name: name, Path: path, Size: p.Size})
		}
	}
	return
}

func writeN(path string, r io.Reader, n int64) error {
	f, err := os.OpenFile(path, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
	if err != nil {
		return err
	}
	if _, err := io.CopyN(f, r, n); err != nil {
		_ = f.Close()
		return err
	}
	return f.Close()
}

// SafeName reduces a peer-supplied file name to a plain base name, so a
// hostile peer cannot write outside the inbox.
func SafeName(name string) string {
	name = strings.NewReplacer("\\", "/", "\x00", "").Replace(name)
	name = filepath.Base(filepath.FromSlash(name))
	if name == "." || name == ".." || name == string(filepath.Separator) || name == "" {
		return "file"
	}
	return name
}
