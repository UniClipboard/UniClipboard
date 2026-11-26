package nativeboard

import (
	"errors"
	"net/url"
	"path/filepath"
	"strings"
	"time"

	"github.com/jezek/xgb"
	"github.com/jezek/xgb/xproto"

	"github.com/uniclipboard/unic/internal/clip"
)

// x11Clipboard owns an unmapped requestor or selection owner for CLIPBOARD.
type x11Clipboard struct {
	conn                      *xgb.Conn
	window                    xproto.Window
	selection, incr, property xproto.Atom
	chunkSize                 int
}

func openX11Clipboard() (*x11Clipboard, error) {
	conn := sourceX11()
	if conn == nil {
		return nil, ErrDesktopClipboardUnsupported
	}
	setup := xproto.Setup(conn)
	window, err := xproto.NewWindowId(conn)
	if err != nil {
		conn.Close()
		return nil, err
	}
	err = xproto.CreateWindowChecked(conn, 0, window, setup.DefaultScreen(conn).Root, 0, 0, 1, 1, 0,
		xproto.WindowClassInputOnly, 0, xproto.CwEventMask, []uint32{xproto.EventMaskPropertyChange}).Check()
	if err != nil {
		conn.Close()
		return nil, err
	}
	c := &x11Clipboard{conn: conn, window: window, chunkSize: int(setup.MaximumRequestLength)*4 - 32}
	c.selection, c.incr, c.property = c.atom("CLIPBOARD"), c.atom("INCR"), c.atom("_UNIC_CLIPBOARD")
	if c.selection == 0 || c.incr == 0 || c.property == 0 || c.chunkSize <= 0 {
		conn.Close()
		return nil, errors.New("X11 clipboard protocol is unavailable")
	}
	return c, nil
}

func (c *x11Clipboard) atom(name string) xproto.Atom {
	reply, err := xproto.InternAtom(c.conn, false, uint16(len(name)), name).Reply()
	if err != nil {
		return 0
	}
	return reply.Atom
}

func (c *x11Clipboard) read(target xproto.Atom, format uint8) ([]byte, error) {
	err := xproto.ConvertSelectionChecked(c.conn, c.window, c.selection, target, c.property, xproto.TimeCurrentTime).Check()
	if err != nil {
		return nil, err
	}
	for {
		event, err := c.conn.WaitForEvent()
		if err != nil || event == nil {
			return nil, errors.New("X11 clipboard selection was interrupted")
		}
		notice, ok := event.(xproto.SelectionNotifyEvent)
		if ok && notice.Requestor == c.window && notice.Target == target {
			if notice.Property == 0 {
				return nil, nil
			}
			break
		}
	}
	reply, err := c.readProperty()
	if err != nil {
		return nil, err
	}
	if reply.Type != c.incr {
		if reply.Format != format {
			return nil, nil
		}
		return reply.Value, nil
	}
	// ICCCM INCR transfers start with a size hint. Deleting each property
	// acknowledges it and lets the owner publish the next chunk.
	var data []byte
	for {
		event, err := c.conn.WaitForEvent()
		if err != nil || event == nil {
			return nil, errors.New("X11 incremental selection was interrupted")
		}
		change, ok := event.(xproto.PropertyNotifyEvent)
		if !ok || change.Atom != c.property || change.Window != c.window || change.State != xproto.PropertyNewValue {
			continue
		}
		reply, readErr := c.readProperty()
		if readErr != nil {
			return nil, readErr
		}
		if reply.Format != format || len(reply.Value) > maxRepresentation-len(data) {
			return nil, errors.New("invalid or oversized X11 clipboard representation")
		}
		if len(reply.Value) == 0 {
			return data, nil
		}
		data = append(data, reply.Value...)
	}
}

func (c *x11Clipboard) readProperty() (*xproto.GetPropertyReply, error) {
	reply, err := xproto.GetProperty(c.conn, true, c.window, c.property, xproto.AtomAny, 0, maxRepresentation/4).Reply()
	if err == nil && reply.BytesAfter != 0 {
		err = errors.New("oversized X11 clipboard property")
	}
	return reply, err
}

var x11Formats = []struct {
	mime    string
	targets []string
}{
	{clip.Text, []string{"UTF8_STRING", "text/plain;charset=utf-8", "text/plain"}},
	{clip.HTML, []string{clip.HTML}},
	{clip.PNG, []string{clip.PNG}},
}

func readX11Clipboard() (clip.Snapshot, error) {
	revision, source := ClipboardSource()
	if revision == 0 {
		return clip.Snapshot{}, nil
	}
	c, err := openX11Clipboard()
	if err != nil {
		return clip.Snapshot{}, err
	}
	defer c.conn.Close()
	timeout := time.AfterFunc(2*time.Second, c.conn.Close)
	defer timeout.Stop()
	out := clip.Snapshot{SourceApplication: source}
	targets, err := c.read(c.atom("TARGETS"), 32)
	if err != nil {
		return clip.Snapshot{}, err
	}
	offered := make(map[xproto.Atom]bool)
	for len(targets) >= 4 {
		offered[xproto.Atom(xgb.Get32(targets))] = true
		targets = targets[4:]
	}
	read := func(name string) ([]byte, error) {
		atom := c.atom(name)
		if len(offered) > 0 && !offered[atom] {
			return nil, nil
		}
		return c.read(atom, 8)
	}
	for _, target := range []string{"text/uri-list", "x-special/gnome-copied-files"} {
		data, err := read(target)
		if err != nil {
			return clip.Snapshot{}, err
		}
		var paths []string
		for line := range strings.Lines(string(data)) {
			u, err := url.Parse(strings.TrimSpace(line))
			if err == nil && u.Scheme == "file" && (u.Host == "" || u.Host == "localhost") && filepath.IsAbs(u.Path) {
				paths = append(paths, u.Path)
			}
		}
		out.Files = clipboardFiles(paths)
		if len(out.Files) != 0 {
			break
		}
	}
	if len(out.Files) == 0 {
		for _, spec := range x11Formats {
			for _, name := range spec.targets {
				data, err := read(name)
				if err != nil {
					return clip.Snapshot{}, err
				}
				if len(data) != 0 {
					out.Parts = append(out.Parts, clip.Part{Format: spec.mime, Data: data})
					break
				}
			}
		}
	}
	if revision != ClipboardOwnerRevision() {
		return clip.Snapshot{}, errors.New("X11 clipboard changed during read")
	}
	return out, nil
}

func writeX11Clipboard(s clip.Snapshot) error {
	c, err := openX11Clipboard()
	if err != nil {
		return err
	}
	data := make(map[xproto.Atom][]byte)
	targets := c.atom("TARGETS")
	var offered []byte
	add := func(atom xproto.Atom, value []byte) {
		offset := len(offered)
		offered = append(offered, 0, 0, 0, 0)
		xgb.Put32(offered[offset:], uint32(atom))
		data[atom] = value
	}
	add(targets, nil)
	if len(s.Files) != 0 {
		var lines []string
		for _, f := range s.Files {
			u := url.URL{Scheme: "file", Path: f.Path}
			lines = append(lines, u.String())
		}
		add(c.atom("text/uri-list"), []byte(strings.Join(lines, "\r\n")+"\r\n"))
		add(c.atom("x-special/gnome-copied-files"), []byte("copy\n"+strings.Join(lines, "\n")))
	} else {
		for _, part := range s.Parts {
			for _, spec := range x11Formats {
				if spec.mime == part.Format {
					for _, target := range spec.targets {
						add(c.atom(target), part.Data)
					}
				}
			}
		}
	}
	data[targets] = offered
	err = xproto.SetSelectionOwnerChecked(c.conn, c.window, c.selection, xproto.TimeCurrentTime).Check()
	if err == nil && selectionOwner(c.conn) != c.window {
		err = errors.New("X11 clipboard ownership was lost")
	}
	if err != nil {
		c.conn.Close()
		return err
	}
	go c.serve(targets, data)
	return nil
}

type x11TransferKey struct {
	window   xproto.Window
	property xproto.Atom
}

type x11Transfer struct {
	target xproto.Atom
	data   []byte
	until  time.Time
}

func (c *x11Clipboard) serve(targets xproto.Atom, data map[xproto.Atom][]byte) {
	defer c.conn.Close()
	pending := make(map[x11TransferKey]x11Transfer)
	for {
		e, err := c.conn.WaitForEvent()
		if err != nil || e == nil {
			return
		}
		now := time.Now()
		for key, transfer := range pending {
			if now.After(transfer.until) {
				delete(pending, key)
			}
		}
		switch e := e.(type) {
		case xproto.SelectionClearEvent:
			return
		case xproto.SelectionRequestEvent:
			property := e.Property
			if property == 0 {
				property = e.Target
			}
			key := x11TransferKey{e.Requestor, property}
			payload, ok := data[e.Target]
			if !ok || c.publish(key, e.Target, targets, payload, pending) != nil {
				property = 0
			}
			reply := xproto.SelectionNotifyEvent{Time: e.Time, Requestor: e.Requestor, Selection: e.Selection, Target: e.Target, Property: property}
			_ = xproto.SendEventChecked(c.conn, false, e.Requestor, 0, string(reply.Bytes())).Check()
		case xproto.PropertyNotifyEvent:
			key := x11TransferKey{e.Window, e.Atom}
			if transfer, ok := pending[key]; ok && e.State == xproto.PropertyDelete {
				n := min(c.chunkSize, len(transfer.data))
				err := c.setProperty(key, transfer.target, 8, transfer.data[:n])
				if n == 0 || err != nil {
					delete(pending, key)
				} else {
					transfer.data = transfer.data[n:]
					transfer.until = now.Add(5 * time.Second)
					pending[key] = transfer
				}
			}
		}
	}
}

func (c *x11Clipboard) publish(key x11TransferKey, target, targets xproto.Atom, payload []byte, pending map[x11TransferKey]x11Transfer) error {
	if target == targets {
		return c.setProperty(key, xproto.AtomAtom, 32, payload)
	}
	if len(payload) <= c.chunkSize {
		return c.setProperty(key, target, 8, payload)
	}
	if len(pending) >= 16 {
		return errors.New("too many X11 clipboard transfers")
	}
	err := xproto.ChangeWindowAttributesChecked(c.conn, key.window, xproto.CwEventMask, []uint32{xproto.EventMaskPropertyChange}).Check()
	if err != nil {
		return err
	}
	hint := make([]byte, 4)
	xgb.Put32(hint, uint32(len(payload)))
	if err := c.setProperty(key, c.incr, 32, hint); err != nil {
		return err
	}
	pending[key] = x11Transfer{target: target, data: payload, until: time.Now().Add(5 * time.Second)}
	return nil
}

func (c *x11Clipboard) setProperty(key x11TransferKey, kind xproto.Atom, format uint8, payload []byte) error {
	return xproto.ChangePropertyChecked(c.conn, xproto.PropModeReplace, key.window, key.property, kind,
		format, uint32(len(payload))/(uint32(format)/8), payload).Check()
}
