// Package rpc implements the local plugin RPC protocol v1 described in
// docs/contracts/plugin-rpc-contract.md: a 32-byte framed transport over a pair
// of pipes, strict JSON control payloads, multiplexed requests with
// cancellation and credit-based binary streams. It does not import the engine.
package rpc

import (
	"encoding/binary"
	"errors"
	"fmt"
	"io"
)

const (
	HeaderBytes     = 32
	ProtocolVersion = 1
	// HandshakeMaxBytes bounds HELLO / HELLO_ACK payloads.
	HandshakeMaxBytes = 16 << 10
)

// Frame types.
const (
	TypeRequest  uint8 = 1
	TypeResponse uint8 = 2
	TypeCancel   uint8 = 3
	TypeData     uint8 = 4
	TypeWindow   uint8 = 5
	TypeEnd      uint8 = 6
	TypeHello    uint8 = 7
	TypeHelloAck uint8 = 8
)

var magic = [4]byte{'U', 'N', 'R', 'P'}

// ErrProtocol marks a violation that ends the session.
var ErrProtocol = errors.New("rpc: protocol error")

func protoErr(format string, args ...any) error {
	return fmt.Errorf("%w: %s", ErrProtocol, fmt.Sprintf(format, args...))
}

// Frame is one decoded protocol frame.
type Frame struct {
	Type      uint8
	RequestID uint64
	StreamID  uint64
	Payload   []byte
}

func knownType(t uint8) bool { return t >= TypeRequest && t <= TypeHelloAck }

// payloadLimit is the largest payload accepted for a frame type.
func (l Limits) payloadLimit(t uint8) uint32 {
	switch t {
	case TypeData:
		return uint32(l.DataBytes)
	case TypeCancel:
		return 0
	case TypeHello, TypeHelloAck:
		return min(uint32(l.ControlBytes), HandshakeMaxBytes)
	default:
		return uint32(l.ControlBytes)
	}
}

// ReadFrame reads exactly one frame. The header is validated and the payload
// length checked against lim before any payload memory is allocated; reads may
// be split or merged arbitrarily by the underlying reader.
func ReadFrame(r io.Reader, lim Limits) (Frame, error) {
	var h [HeaderBytes]byte
	if _, err := io.ReadFull(r, h[:]); err != nil {
		return Frame{}, err
	}
	if [4]byte(h[0:4]) != magic {
		return Frame{}, protoErr("bad magic")
	}
	if binary.BigEndian.Uint16(h[4:6]) != ProtocolVersion {
		return Frame{}, protoErr("unsupported version")
	}
	f := Frame{
		Type:      h[6],
		RequestID: binary.BigEndian.Uint64(h[8:16]),
		StreamID:  binary.BigEndian.Uint64(h[16:24]),
	}
	n := binary.BigEndian.Uint32(h[24:28])
	switch {
	case h[7] != 0:
		return Frame{}, protoErr("nonzero flags")
	case binary.BigEndian.Uint32(h[28:32]) != 0:
		return Frame{}, protoErr("nonzero reserved bits")
	case !knownType(f.Type):
		return Frame{}, protoErr("unknown frame type")
	case n > lim.payloadLimit(f.Type):
		return Frame{}, protoErr("payload too large")
	}
	if err := f.checkIDs(n); err != nil {
		return Frame{}, err
	}
	if n > 0 {
		f.Payload = make([]byte, n)
		if _, err := io.ReadFull(r, f.Payload); err != nil {
			if errors.Is(err, io.EOF) {
				err = io.ErrUnexpectedEOF
			}
			return Frame{}, err
		}
	}
	return f, nil
}

func (f Frame) checkIDs(n uint32) error {
	switch f.Type {
	case TypeRequest:
		if f.RequestID == 0 || f.StreamID != 0 {
			return protoErr("bad request ids")
		}
	case TypeResponse:
		if f.RequestID == 0 || f.StreamID != 0 {
			return protoErr("bad response ids")
		}
	case TypeCancel:
		if f.RequestID == 0 || f.StreamID != 0 || n != 0 {
			return protoErr("bad cancel frame")
		}
	case TypeData, TypeWindow, TypeEnd:
		if f.RequestID == 0 || f.StreamID == 0 {
			return protoErr("bad stream ids")
		}
		if f.Type == TypeData && n == 0 {
			return protoErr("empty data frame")
		}
	case TypeHello, TypeHelloAck:
		if f.RequestID != 0 || f.StreamID != 0 {
			return protoErr("bad handshake ids")
		}
	}
	return nil
}

// AppendFrame appends the wire form of f to dst.
func AppendFrame(dst []byte, f Frame) []byte {
	var h [HeaderBytes]byte
	copy(h[0:4], magic[:])
	binary.BigEndian.PutUint16(h[4:6], ProtocolVersion)
	h[6] = f.Type
	binary.BigEndian.PutUint64(h[8:16], f.RequestID)
	binary.BigEndian.PutUint64(h[16:24], f.StreamID)
	binary.BigEndian.PutUint32(h[24:28], uint32(len(f.Payload)))
	dst = append(dst, h[:]...)
	return append(dst, f.Payload...)
}

// WriteFrame writes f with a single Write so frames never interleave on a
// writer that is used by one goroutine at a time.
func WriteFrame(w io.Writer, f Frame) error {
	_, err := w.Write(AppendFrame(make([]byte, 0, HeaderBytes+len(f.Payload)), f))
	return err
}
