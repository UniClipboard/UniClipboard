// Package webtransfer speaks the browser transfer protocol of try.uniclipboard.app
// (uc-website/src/lib/web-transfer). One side offers items, the other pulls them,
// and the offering side reports delivery only after the puller acknowledged every
// SHA-256. The package knows nothing about unic devices, history or tailcat: it
// works on any net.Conn.
//
// Tokens, addresses, codes and links are secrets. Nothing here logs them.
package webtransfer

import (
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"
)

// Frame types, identical to the browser implementation.
const (
	frameHello  byte = 1
	frameOffer  byte = 2
	frameBusy   byte = 3
	frameReject byte = 4
	frameData   byte = 5
	frameEnd    byte = 6
	frameAck    byte = 7
	frameCancel byte = 8
)

const (
	headerBytes = 5
	// MaxDataBytes is the largest DATA payload.
	MaxDataBytes = 64 * 1024
	// maxControlBytes bounds every JSON control frame.
	maxControlBytes = 64 * 1024
)

// maxPayload is the per-type ceiling checked before a payload is allocated.
var maxPayload = map[byte]uint32{
	frameHello: maxControlBytes, frameOffer: maxControlBytes, frameBusy: 0, frameReject: maxControlBytes,
	frameData: MaxDataBytes, frameEnd: maxControlBytes, frameAck: maxControlBytes, frameCancel: 0,
}

var errFrame = errors.New("webtransfer: malformed frame")

type frame struct {
	typ     byte
	payload []byte
}

func writeFrame(w io.Writer, typ byte, payload []byte) error {
	if uint32(len(payload)) > maxPayload[typ] {
		return errFrame
	}
	buf := make([]byte, headerBytes+len(payload))
	buf[0] = typ
	binary.BigEndian.PutUint32(buf[1:], uint32(len(payload)))
	copy(buf[headerBytes:], payload)
	_, err := w.Write(buf)
	return err
}

func writeControl(w io.Writer, typ byte, msg any) error {
	b, err := json.Marshal(msg)
	if err != nil {
		return err
	}
	return writeFrame(w, typ, b)
}

// readFrame returns io.EOF only at a frame boundary.
func readFrame(r io.Reader) (frame, error) {
	var h [headerBytes]byte
	if _, err := io.ReadFull(r, h[:]); err != nil {
		if errors.Is(err, io.ErrUnexpectedEOF) {
			return frame{}, errFrame
		}
		return frame{}, err
	}
	limit, known := maxPayload[h[0]]
	n := binary.BigEndian.Uint32(h[1:])
	if !known || n > limit {
		return frame{}, fmt.Errorf("%w: type %d length %d", errFrame, h[0], n)
	}
	p := make([]byte, n)
	if _, err := io.ReadFull(r, p); err != nil {
		return frame{}, errFrame
	}
	return frame{h[0], p}, nil
}

func decodeControl(f frame) (map[string]any, error) {
	var m map[string]any
	if json.Unmarshal(f.payload, &m) != nil || m == nil {
		return nil, errFrame
	}
	return m, nil
}
