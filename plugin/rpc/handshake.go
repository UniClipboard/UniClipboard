package rpc

import (
	"context"
	"crypto/rand"
	"crypto/subtle"
	"encoding/hex"
	"io"
	"time"
)

// DefaultHandshakeTimeout bounds the HELLO exchange.
const DefaultHandshakeTimeout = 5 * time.Second

// Duplex joins a read half and a write half into one closable stream, e.g. a
// child's stdout and stdin.
type Duplex struct {
	io.Reader
	io.Writer
	closers []io.Closer
}

// NewDuplex returns a Duplex that closes every non-nil closer on Close.
func NewDuplex(r io.Reader, w io.Writer, closers ...io.Closer) *Duplex {
	return &Duplex{Reader: r, Writer: w, closers: closers}
}

func (d *Duplex) Close() error {
	var first error
	for _, c := range d.closers {
		if err := c.Close(); err != nil && first == nil {
			first = err
		}
	}
	return first
}

// HostHandshake sends HELLO, waits for HELLO_ACK and checks that the plugin
// is the one expected. It returns the plugin's Spec and the negotiated
// limits. On timeout or cancellation it returns an error; the caller must
// close the stream, which unblocks the pending read.
func HostHandshake(ctx context.Context, rw io.ReadWriter, expected Spec, offer Limits, timeout time.Duration) (Limits, error) {
	if timeout <= 0 {
		timeout = DefaultHandshakeTimeout
	}
	if err := offer.Validate(); err != nil {
		return Limits{}, err
	}
	var raw [32]byte
	if _, err := rand.Read(raw[:]); err != nil {
		return Limits{}, err
	}
	nonce := hex.EncodeToString(raw[:])
	ctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()

	type result struct {
		lim Limits
		err error
	}
	ch := make(chan result, 1)
	go func() {
		payload, err := Encode(Hello{Protocol: ProtocolVersion, Nonce: nonce, ExpectedSpec: expected, Limits: offer})
		if err == nil {
			err = WriteFrame(rw, Frame{Type: TypeHello, Payload: payload})
		}
		if err != nil {
			ch <- result{err: err}
			return
		}
		f, err := ReadFrame(rw, offer)
		if err != nil {
			ch <- result{err: err}
			return
		}
		if f.Type != TypeHelloAck {
			ch <- result{err: protoErr("expected HELLO_ACK")}
			return
		}
		var ack HelloAck
		if err := Decode(f.Payload, &ack); err != nil {
			ch <- result{err: err}
			return
		}
		switch {
		case ack.Protocol != ProtocolVersion:
			ch <- result{err: NewError("protocol_mismatch")}
		case subtle.ConstantTimeCompare([]byte(ack.Nonce), []byte(nonce)) != 1:
			ch <- result{err: protoErr("nonce mismatch")}
		case !ack.Spec.Equal(expected):
			ch <- result{err: NewError("identity_mismatch")}
		default:
			lim := ack.Limits
			if err := lim.Validate(); err != nil || Negotiate(lim, offer) != lim {
				ch <- result{err: protoErr("limits not acceptable")}
				return
			}
			ch <- result{lim: lim}
		}
	}()
	select {
	case r := <-ch:
		return r.lim, r.err
	case <-ctx.Done():
		return Limits{}, toError(ctx.Err())
	}
}

// PluginHandshake waits for HELLO, checks that the host expects exactly self
// and answers with the negotiated limits.
func PluginHandshake(rw io.ReadWriter, self Spec, supported Limits) (Limits, error) {
	f, err := ReadFrame(rw, DefaultLimits)
	if err != nil {
		return Limits{}, err
	}
	if f.Type != TypeHello {
		return Limits{}, protoErr("expected HELLO")
	}
	var h Hello
	if err := Decode(f.Payload, &h); err != nil {
		return Limits{}, err
	}
	if h.Protocol != ProtocolVersion {
		return Limits{}, NewError("protocol_mismatch")
	}
	if len(h.Nonce) != 64 || h.Limits.Validate() != nil {
		return Limits{}, protoErr("bad hello")
	}
	if !h.ExpectedSpec.Equal(self) {
		return Limits{}, NewError("identity_mismatch")
	}
	lim := Negotiate(h.Limits, supported)
	payload, err := Encode(HelloAck{Protocol: ProtocolVersion, Nonce: h.Nonce, Spec: self, Limits: lim})
	if err != nil {
		return Limits{}, err
	}
	if err := WriteFrame(rw, Frame{Type: TypeHelloAck, Payload: payload}); err != nil {
		return Limits{}, err
	}
	return lim, nil
}
