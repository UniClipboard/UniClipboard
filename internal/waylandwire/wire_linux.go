// Package waylandwire discovers Wayland globals and sends requests on the user display socket.
package waylandwire

import (
	"encoding/binary"
	"errors"
	"net"
	"os"
	"path/filepath"
	"strings"
	"time"
)

const (
	DisplayID      uint32 = 1
	OpSync         uint16 = 0
	OpGetRegistry  uint16 = 1
	registryBind   uint16 = 0
	RegistryGlobal uint16 = 0
	CallbackDone   uint16 = 0
)

type Global struct{ Name, Version uint32 }

func SocketPath() string {
	display := strings.TrimSpace(os.Getenv("WAYLAND_DISPLAY"))
	if display == "" {
		return ""
	}
	if filepath.IsAbs(display) {
		return display
	}
	runtimeDir := strings.TrimSpace(os.Getenv("XDG_RUNTIME_DIR"))
	if runtimeDir == "" {
		return ""
	}
	return filepath.Join(runtimeDir, display)
}

type Conn struct {
	uc     *net.UnixConn
	nextID uint32
	buf    []byte
}

func Dial(path string) (*Conn, error) {
	conn, err := net.DialTimeout("unix", path, time.Second)
	if err != nil {
		return nil, err
	}
	uc, ok := conn.(*net.UnixConn)
	if !ok {
		_ = conn.Close()
		return nil, errors.New("wayland: non-unix socket")
	}
	_ = uc.SetDeadline(time.Now().Add(2 * time.Second))
	return &Conn{uc: uc, nextID: 2}, nil
}

func (c *Conn) AllocateID() uint32 {
	id := c.nextID
	c.nextID++
	return id
}

func (c *Conn) Close() {
	if c.uc != nil {
		_ = c.uc.Close()
	}
}

func (c *Conn) Send(objectID uint32, opcode uint16, payload []byte, oob []byte) error {
	size := 8 + len(payload)
	if size > 0xffff {
		return errors.New("wayland: message too large")
	}
	msg := make([]byte, size)
	binary.LittleEndian.PutUint32(msg[0:4], objectID)
	binary.LittleEndian.PutUint32(msg[4:8], (uint32(size)<<16)|uint32(opcode))
	copy(msg[8:], payload)
	n, oobn, err := c.uc.WriteMsgUnix(msg, oob, nil)
	if err != nil {
		return err
	}
	if n != len(msg) || oobn != len(oob) {
		return errors.New("wayland: short write")
	}
	return nil
}

func (c *Conn) BindGlobal(regID, name uint32, iface string, version, newID uint32) error {
	s := EncodeString(iface)
	payload := make([]byte, 4+len(s)+8)
	binary.LittleEndian.PutUint32(payload[0:4], name)
	copy(payload[4:], s)
	off := 4 + len(s)
	binary.LittleEndian.PutUint32(payload[off:off+4], version)
	binary.LittleEndian.PutUint32(payload[off+4:off+8], newID)
	return c.Send(regID, registryBind, payload, nil)
}

func (c *Conn) DiscoverGlobals() (map[string]Global, error) {
	regID := c.AllocateID()
	syncID := c.AllocateID()
	if err := c.Send(DisplayID, OpGetRegistry, EncodeU32s(regID), nil); err != nil {
		return nil, err
	}
	if err := c.Send(DisplayID, OpSync, EncodeU32s(syncID), nil); err != nil {
		return nil, err
	}
	globals := map[string]Global{}
	err := c.readUntilCallback(syncID, func(oid uint32, opcode uint16, payload []byte) {
		if oid == regID && opcode == RegistryGlobal {
			if name, iface, ver, ok := decodeGlobalEvent(payload); ok {
				globals[iface] = Global{Name: name, Version: ver}
			}
		}
	})
	return globals, err
}

func (c *Conn) Roundtrip() error {
	cbID := c.nextID
	c.nextID++
	if err := c.Send(DisplayID, OpSync, EncodeU32s(cbID), nil); err != nil {
		return err
	}
	return c.readUntilCallback(cbID, nil)
}

func (c *Conn) readUntilCallback(cbID uint32, onEvent func(uint32, uint16, []byte)) error {
	tmp := make([]byte, 4096)
	oob := make([]byte, 256)
	for {
		for len(c.buf) >= 8 {
			oid := binary.LittleEndian.Uint32(c.buf[0:4])
			szOp := binary.LittleEndian.Uint32(c.buf[4:8])
			size := int(szOp >> 16)
			opcode := uint16(szOp & 0xffff)
			if size < 8 {
				return errors.New("wayland: invalid message size")
			}
			if len(c.buf) < size {
				break
			}
			payload := c.buf[8:size]
			c.buf = c.buf[size:]
			if oid == DisplayID && opcode == 0 {
				return errors.New("wayland: protocol error")
			}
			if oid == cbID && opcode == CallbackDone {
				return nil
			}
			if onEvent != nil {
				onEvent(oid, opcode, payload)
			}
		}
		n, _, _, _, err := c.uc.ReadMsgUnix(tmp, oob)
		if err != nil {
			return err
		}
		if n == 0 {
			return errors.New("wayland: eof")
		}
		c.buf = append(c.buf, tmp[:n]...)
	}
}

func EncodeU32s(vals ...uint32) []byte {
	out := make([]byte, 4*len(vals))
	for i, v := range vals {
		binary.LittleEndian.PutUint32(out[4*i:4*i+4], v)
	}
	return out
}

func EncodeString(s string) []byte {
	n := len(s) + 1
	padded := (n + 3) &^ 3
	out := make([]byte, 4+padded)
	binary.LittleEndian.PutUint32(out[0:4], uint32(n))
	copy(out[4:], s)
	return out
}

func decodeGlobalEvent(payload []byte) (name uint32, iface string, version uint32, ok bool) {
	if len(payload) < 12 {
		return 0, "", 0, false
	}
	name = binary.LittleEndian.Uint32(payload[0:4])
	slen := int(binary.LittleEndian.Uint32(payload[4:8]))
	if slen <= 0 {
		return 0, "", 0, false
	}
	padded := (slen + 3) &^ 3
	if 8+padded+4 > len(payload) {
		return 0, "", 0, false
	}
	iface = strings.TrimRight(string(payload[8:8+slen]), "\x00")
	version = binary.LittleEndian.Uint32(payload[8+padded : 12+padded])
	return name, iface, version, true
}
