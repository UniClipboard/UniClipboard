package rpc

import (
	"context"
	"io"
)

// RecvStream receives a binary stream. Credit is granted as the reader
// consumes data, so a slow reader applies back-pressure to the sender.
type RecvStream struct {
	c        *Conn
	ctx      context.Context
	id, root uint64
	size     uint64

	// guarded by c.mu
	buf                         []byte
	granted, received, consumed uint64
	ended                       bool
	failed                      error
	released                    bool
}

// SendStream sends a binary stream, blocking while it has no credit.
type SendStream struct {
	c        *Conn
	ctx      context.Context
	id, root uint64
	size     uint64

	// guarded by c.mu
	credit, granted, sent uint64
	ended                 bool
	failed                error
}

func (c *Conn) activeStreamsLocked() int {
	n := 0
	for _, s := range c.recv {
		if !s.ended {
			n++
		}
	}
	for _, s := range c.send {
		if !s.ended {
			n++
		}
	}
	return n
}

func (c *Conn) noteStreamLocked(id uint64) {
	if p := id & 1; id > c.maxStream[p] {
		c.maxStream[p] = id
	}
}

// NewSend allocates a stream this side will send, rooted at the request
// root. Its description goes into the request that announces it.
func (c *Conn) NewSend(ctx context.Context, root uint64, purpose string, size uint64) (*SendStream, StreamDesc, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.err != nil {
		return nil, StreamDesc{}, NewError(CodeUnavailable)
	}
	if c.activeStreamsLocked() >= c.lim.MaxStreams {
		return nil, StreamDesc{}, NewError(CodeExhausted)
	}
	id := c.nextStream
	c.nextStream += 2
	c.noteStreamLocked(id)
	s := &SendStream{c: c, ctx: ctx, id: id, root: root, size: size}
	c.send[id] = s
	return s, StreamDesc{StreamID: U64(id), Size: U64(size), Purpose: purpose}, nil
}

// Expect registers a stream the peer will send, granting its initial credit.
// maxSize rejects larger descriptions before anything is reserved.
func (c *Conn) Expect(ctx context.Context, root uint64, d StreamDesc, maxSize uint64) (*RecvStream, error) {
	id := uint64(d.StreamID)
	if id == 0 || id&1 != c.peerParity() || uint64(d.Size) > maxSize {
		return nil, NewError(CodeInvalidArgument)
	}
	c.mu.Lock()
	if c.err != nil {
		c.mu.Unlock()
		return nil, NewError(CodeUnavailable)
	}
	if c.recv[id] != nil || c.send[id] != nil || id <= c.maxStream[id&1] {
		c.mu.Unlock()
		return nil, NewError(CodeInvalidState)
	}
	if c.activeStreamsLocked() >= c.lim.MaxStreams {
		c.mu.Unlock()
		return nil, NewError(CodeExhausted)
	}
	c.noteStreamLocked(id)
	s := &RecvStream{c: c, ctx: ctx, id: id, root: root, size: uint64(d.Size)}
	c.recv[id] = s
	frames := c.grantLocked()
	c.mu.Unlock()
	c.sendAll(frames)
	return s, nil
}

// grantLocked tops up every receive stream within the per-stream window and
// the session budget and returns the WINDOW frames to send.
func (c *Conn) grantLocked() []Frame {
	var out []Frame
	for _, s := range c.recv {
		if s.ended || s.released || s.failed != nil {
			continue
		}
		room := uint64(c.lim.StreamWindowBytes) - (s.granted - s.consumed)
		room = min(room, s.size-s.granted, uint64(SessionStreamBudget-c.budgetUsed))
		if room == 0 {
			continue
		}
		s.granted += room
		c.budgetUsed += int(room)
		p, _ := Encode(Window{CreditBytes: U64(room)})
		out = append(out, Frame{Type: TypeWindow, RequestID: s.root, StreamID: s.id, Payload: p})
	}
	return out
}

func (c *Conn) sendAll(frames []Frame) {
	for _, f := range frames {
		if c.write(f) != nil {
			return
		}
	}
}

// Read implements io.Reader; it returns io.EOF after a complete END.
func (s *RecvStream) Read(p []byte) (int, error) {
	if len(p) == 0 {
		return 0, nil
	}
	c := s.c
	c.mu.Lock()
	for {
		switch {
		case len(s.buf) > 0:
			n := copy(p, s.buf)
			s.buf = s.buf[n:]
			s.consumed += uint64(n)
			c.budgetUsed -= n
			frames := c.grantLocked()
			c.mu.Unlock()
			c.sendAll(frames)
			return n, nil
		case s.failed != nil:
			c.mu.Unlock()
			return 0, s.failed
		case s.ended:
			c.mu.Unlock()
			return 0, io.EOF
		}
		if err := c.waitLocked(s.ctx); err != nil {
			c.mu.Unlock()
			return 0, err
		}
	}
}

// Close releases the stream and any credit it still holds.
func (s *RecvStream) Close() error {
	c := s.c
	c.mu.Lock()
	c.releaseRecvLocked(s, NewError(CodeCancelled))
	frames := c.grantLocked()
	c.broadcastLocked()
	c.mu.Unlock()
	c.sendAll(frames)
	return nil
}

func (c *Conn) releaseRecvLocked(s *RecvStream, why error) {
	if s.released {
		return
	}
	s.released = true
	c.budgetUsed -= int(s.granted - s.consumed)
	s.consumed = s.granted
	s.buf = nil
	if !s.ended && s.failed == nil {
		s.failed = why
	}
	s.ended = true
	delete(c.recv, s.id)
}

// ReleaseRoot drops every stream rooted at the request: receivers are failed,
// senders stop. It is idempotent.
func (c *Conn) ReleaseRoot(root uint64) {
	c.mu.Lock()
	for _, s := range c.recv {
		if s.root == root {
			c.releaseRecvLocked(s, NewError(CodeCancelled))
		}
	}
	for id, s := range c.send {
		if s.root == root {
			if !s.ended && s.failed == nil {
				s.failed = NewError(CodeCancelled)
			}
			s.ended = true
			delete(c.send, id)
		}
	}
	frames := c.grantLocked()
	c.broadcastLocked()
	c.mu.Unlock()
	c.sendAll(frames)
}

// Write sends p, waiting for credit as needed.
func (s *SendStream) Write(p []byte) (int, error) {
	c := s.c
	total := 0
	c.mu.Lock()
	if s.failed == nil && !s.ended && uint64(len(p)) > s.size-s.sent {
		c.mu.Unlock()
		return 0, NewError(CodeInvalidArgument)
	}
	c.mu.Unlock()
	for len(p) > 0 {
		c.mu.Lock()
		for s.credit == 0 && s.failed == nil && !s.ended {
			if err := c.waitLocked(s.ctx); err != nil {
				c.mu.Unlock()
				return total, err
			}
		}
		if s.failed != nil || s.ended {
			err := s.failed
			if err == nil {
				err = NewError(CodeInvalidState)
			}
			c.mu.Unlock()
			return total, err
		}
		n := min(uint64(len(p)), s.credit, uint64(c.lim.DataBytes))
		s.credit -= n
		s.sent += n
		c.mu.Unlock()
		if err := c.write(Frame{Type: TypeData, RequestID: s.root, StreamID: s.id, Payload: p[:n]}); err != nil {
			return total, err
		}
		total += int(n)
		p = p[n:]
	}
	return total, nil
}

// Close ends the stream successfully; it fails if fewer than size bytes were
// written, because a truncated stream must never look complete.
func (s *SendStream) Close() error {
	c := s.c
	c.mu.Lock()
	if s.failed != nil {
		err := s.failed
		c.mu.Unlock()
		return err
	}
	if s.ended {
		c.mu.Unlock()
		return nil
	}
	if s.sent != s.size {
		s.failed = NewError(CodeInvalidState)
		s.ended = true
		delete(c.send, s.id)
		c.mu.Unlock()
		return s.failed
	}
	s.ended = true
	c.mu.Unlock()
	p, _ := Encode(End{OK: true, Size: U64(s.size)})
	return c.write(Frame{Type: TypeEnd, RequestID: s.root, StreamID: s.id, Payload: p})
}

func (c *Conn) onWindow(f Frame) error {
	var w Window
	if err := Decode(f.Payload, &w); err != nil {
		return err
	}
	if err := w.validate(); err != nil {
		return err
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	s := c.send[f.StreamID]
	if s == nil {
		if c.known(f.StreamID) {
			return c.drainLocked(len(f.Payload))
		}
		return protoErr("window for unknown stream")
	}
	if s.root != f.RequestID {
		return protoErr("stream root mismatch")
	}
	n := uint64(w.CreditBytes)
	if s.credit+n > uint64(c.lim.StreamWindowBytes) || s.granted+n > s.size {
		return protoErr("credit beyond window or size")
	}
	s.credit += n
	s.granted += n
	c.broadcastLocked()
	return nil
}

func (c *Conn) known(id uint64) bool { return id <= c.maxStream[id&1] }

func (c *Conn) onData(f Frame) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	s := c.recv[f.StreamID]
	if s == nil {
		if c.send[f.StreamID] != nil {
			return protoErr("data on a stream this side sends")
		}
		if c.known(f.StreamID) {
			return c.drainLocked(len(f.Payload))
		}
		return protoErr("data for unknown stream")
	}
	if s.root != f.RequestID {
		return protoErr("stream root mismatch")
	}
	n := uint64(len(f.Payload))
	if s.ended || s.failed != nil || n > s.granted-s.received || s.received+n > s.size {
		return protoErr("data beyond granted credit")
	}
	s.received += n
	s.buf = append(s.buf, f.Payload...)
	c.broadcastLocked()
	return nil
}

func (c *Conn) onEnd(f Frame) error {
	var e End
	if err := Decode(f.Payload, &e); err != nil {
		return err
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	s := c.recv[f.StreamID]
	if s == nil {
		if c.known(f.StreamID) && c.send[f.StreamID] == nil {
			return c.drainLocked(len(f.Payload))
		}
		return protoErr("end for unknown stream")
	}
	if s.root != f.RequestID || !e.OK || s.ended || uint64(e.Size) != s.size || s.received != s.size {
		return protoErr("stream ended with the wrong size")
	}
	s.ended = true
	c.broadcastLocked()
	return nil
}
