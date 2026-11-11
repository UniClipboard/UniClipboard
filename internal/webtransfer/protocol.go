package webtransfer

import (
	"bytes"
	"context"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"net"
	"sync"
	"time"
)

const (
	helloTimeout = 15 * time.Second
	idleTimeout  = 60 * time.Second
	drainTimeout = 2 * time.Second
)

// Pull dials-side half: it fetches the sender's content over conn. admit sees the
// validated offer before any DATA is read; an error from it answers REJECT.
func Pull(ctx context.Context, conn net.Conn, token string, admit func(Offer) error) ([]Item, error) {
	stop := context.AfterFunc(ctx, func() { _ = conn.Close() })
	defer stop()
	items, err := pull(conn, token, admit)
	if err != nil {
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		_ = writeFrame(conn, frameCancel, nil)
		return nil, err
	}
	drain(conn)
	return items, nil
}

func pull(conn net.Conn, token string, admit func(Offer) error) ([]Item, error) {
	if err := writeControl(conn, frameHello, map[string]any{"v": protocolVersion, "role": "pull", "k": token}); err != nil {
		return nil, fmt.Errorf("webtransfer: connection lost: %w", err)
	}
	_ = conn.SetReadDeadline(time.Now().Add(idleTimeout))
	first, err := readFrame(conn)
	if err != nil {
		return nil, wrapRead(err)
	}
	switch first.typ {
	case frameBusy:
		return nil, ErrBusy
	case frameCancel:
		return nil, ErrCancelled
	case frameReject:
		return nil, ErrRejected
	case frameOffer:
	default:
		return nil, ErrProtocol
	}
	var offer Offer
	if err := decodeInto(first, &offer); err != nil || !validOffer(offer) {
		return nil, ErrProtocol
	}
	if offer.Total > MaxTransferBytes {
		_ = writeControl(conn, frameReject, map[string]string{"reason": "too-large"})
		return nil, ErrTooLarge
	}
	if err := admit(offer); err != nil {
		_ = writeControl(conn, frameReject, map[string]string{"reason": "rejected"})
		return nil, err
	}
	buf := make([]byte, 0, offer.Total)
	for {
		_ = conn.SetReadDeadline(time.Now().Add(idleTimeout))
		f, err := readFrame(conn)
		if err != nil {
			return nil, wrapRead(err)
		}
		switch f.typ {
		case frameData:
			if int64(len(buf)+len(f.payload)) > offer.Total {
				return nil, ErrProtocol
			}
			buf = append(buf, f.payload...)
			continue
		case frameCancel:
			return nil, ErrCancelled
		case frameEnd:
			m, err := decodeControl(f)
			if err != nil || m["transferId"] != offer.TransferID || int64(len(buf)) != offer.Total {
				return nil, ErrProtocol
			}
		default:
			return nil, ErrProtocol
		}
		break
	}
	items := make([]Item, len(offer.Items))
	ok := true
	var off int64
	for i, m := range offer.Items {
		data := buf[off : off+m.Size]
		off += m.Size
		sum := sha256.Sum256(data)
		if hex.EncodeToString(sum[:]) != m.SHA256 {
			ok = false
		}
		items[i] = Item{Meta: m, Data: data}
	}
	if err := writeControl(conn, frameAck, map[string]any{"transferId": offer.TransferID, "ok": ok}); err != nil {
		return nil, fmt.Errorf("webtransfer: connection lost: %w", err)
	}
	if !ok {
		return nil, ErrIntegrity
	}
	return items, nil
}

// Events reports a sender's progress. Both may be nil.
type Events struct {
	// Started runs when a receiver with the right token began its pull.
	Started func(total int64)
}

// Serve offers items to one receiver that presents token, accepted from ln. It
// returns nil once the receiver acknowledged every item. Connections with a wrong
// token or hello are closed silently; a second receiver gets BUSY. The caller
// closes ln; Serve stops when ctx ends or ln fails.
func Serve(ctx context.Context, ln net.Listener, token string, offer Offer, items []Item, ev Events) error {
	s := &sender{token: token, offer: offer, items: items, ev: ev, done: make(chan error, 1)}
	go func() {
		for {
			c, err := ln.Accept()
			if err != nil {
				s.finish(err)
				return
			}
			go s.serve(ctx, c)
		}
	}()
	select {
	case err := <-s.done:
		return err
	case <-ctx.Done():
		s.mu.Lock()
		if s.active != nil {
			_ = s.active.Close()
		}
		s.mu.Unlock()
		return ctx.Err()
	}
}

type sender struct {
	token string
	offer Offer
	items []Item
	ev    Events

	mu     sync.Mutex
	state  string // waiting, sending, delivered
	active net.Conn
	done   chan error
}

func (s *sender) finish(err error) {
	select {
	case s.done <- err:
	default:
	}
}

func (s *sender) serve(ctx context.Context, c net.Conn) {
	defer func() { _ = c.Close() }()
	_ = c.SetReadDeadline(time.Now().Add(helloTimeout))
	f, err := readFrame(c)
	if err != nil || f.typ != frameHello {
		return
	}
	m, err := decodeControl(f)
	if err != nil {
		return
	}
	k, _ := m["k"].(string)
	if v, _ := m["v"].(float64); v != protocolVersion || subtle.ConstantTimeCompare([]byte(k), []byte(s.token)) != 1 {
		return
	}
	role, _ := m["role"].(string)
	s.mu.Lock()
	accept := role == "pull" && s.state == ""
	if accept {
		s.state = "sending"
		s.active = c
	}
	s.mu.Unlock()
	if !accept {
		_ = writeFrame(c, frameBusy, nil)
		return
	}
	if s.ev.Started != nil {
		s.ev.Started(s.offer.Total)
	}
	if err := s.send(c); err != nil {
		_ = writeFrame(c, frameCancel, nil)
		s.finish(err)
		return
	}
	s.mu.Lock()
	s.state = "delivered"
	s.mu.Unlock()
	drain(c)
	s.finish(nil)
}

func (s *sender) send(c net.Conn) error {
	if err := writeControl(c, frameOffer, s.offer); err != nil {
		return fmt.Errorf("webtransfer: connection lost: %w", err)
	}
	// The receiver may answer at any time (REJECT on the offer, CANCEL mid-stream),
	// so its first frame is read concurrently with the writes.
	type verdict struct {
		f   frame
		err error
	}
	got := make(chan verdict, 1)
	go func() {
		_ = c.SetReadDeadline(time.Time{})
		f, err := readFrame(c)
		got <- verdict{f, err}
	}()
	var sent int64
	for _, it := range s.items {
		for off := 0; off < len(it.Data); off += MaxDataBytes {
			select {
			case v := <-got:
				return verdictError(v.f, v.err)
			default:
			}
			end := min(off+MaxDataBytes, len(it.Data))
			if err := writeFrame(c, frameData, it.Data[off:end]); err != nil {
				select {
				case v := <-got:
					return verdictError(v.f, v.err)
				case <-time.After(500 * time.Millisecond):
				}
				return fmt.Errorf("webtransfer: connection lost: %w", err)
			}
			sent += int64(end - off)
		}
	}
	if err := writeControl(c, frameEnd, map[string]string{"transferId": s.offer.TransferID}); err != nil {
		return fmt.Errorf("webtransfer: connection lost: %w", err)
	}
	select {
	case v := <-got:
		if v.err == nil && v.f.typ == frameAck {
			m, err := decodeControl(v.f)
			if err != nil || m["transferId"] != s.offer.TransferID || sent != s.offer.Total {
				return ErrProtocol
			}
			if ok, _ := m["ok"].(bool); !ok {
				return ErrIntegrity
			}
			return nil
		}
		return verdictError(v.f, v.err)
	case <-time.After(idleTimeout):
		return errors.New("webtransfer: timed out waiting for the receiver")
	}
}

func verdictError(f frame, err error) error {
	if err != nil {
		return wrapRead(err)
	}
	switch f.typ {
	case frameCancel:
		return ErrCancelled
	case frameBusy:
		return ErrBusy
	case frameReject:
		return ErrRejected
	}
	return ErrProtocol
}

func wrapRead(err error) error {
	if errors.Is(err, errFrame) {
		return ErrProtocol
	}
	return fmt.Errorf("webtransfer: connection lost: %w", err)
}

func decodeInto(f frame, v any) error {
	d := bytes.NewReader(f.payload)
	return jsonDecode(d, v)
}

// drain lets the peer read our last frame before the stream goes away.
func drain(c net.Conn) {
	if cw, ok := c.(interface{ CloseWrite() error }); ok {
		_ = cw.CloseWrite()
	}
	_ = c.SetReadDeadline(time.Now().Add(drainTimeout))
	_, _ = io.Copy(io.Discard, c)
}
