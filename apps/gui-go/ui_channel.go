package main

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"log"
	"os"
	"sync"
	"sync/atomic"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/hostapi"
)

// uiPacket travels only over inherited private pipes. It is never persisted.
type uiPacket struct {
	ID        uint64          `json:"id,omitempty"`
	Reply     bool            `json:"reply,omitempty"`
	Operation string          `json:"operation,omitempty"`
	Data      json.RawMessage `json:"data,omitempty"`
	Error     json.RawMessage `json:"error,omitempty"`
}

type uiPeer struct {
	reader  io.ReadCloser
	writer  io.WriteCloser
	writeMu sync.Mutex
	mu      sync.Mutex
	pending map[uint64]chan uiPacket
	next    atomic.Uint64
	done    chan struct{}
	handle  func(uiPacket) (any, error)
}

func newUIPeer(reader io.ReadCloser, writer io.WriteCloser, handle func(uiPacket) (any, error)) *uiPeer {
	p := &uiPeer{reader: reader, writer: writer, pending: make(map[uint64]chan uiPacket), done: make(chan struct{}), handle: handle}
	go p.read()
	return p
}
func (p *uiPeer) send(m uiPacket) error {
	p.writeMu.Lock()
	defer p.writeMu.Unlock()
	if file, ok := p.writer.(interface{ SetWriteDeadline(time.Time) error }); ok {
		if err := file.SetWriteDeadline(time.Now().Add(5 * time.Second)); err != nil {
			if !errors.Is(err, os.ErrNoDeadline) {
				return err
			}
		} else {
			defer file.SetWriteDeadline(time.Time{})
		}
	}
	return json.NewEncoder(p.writer).Encode(m)
}
func (p *uiPeer) read() {
	defer close(p.done)
	// Notifications need FIFO ordering. An unbounded pending queue keeps a worker's
	// reverse request from blocking the decoder that must receive its reply.
	var notificationMu sync.Mutex
	var notifications []uiPacket
	wake := make(chan struct{}, 1)
	go func() {
		defer func() { notificationMu.Lock(); notifications = nil; notificationMu.Unlock() }()
		for {
			select {
			case <-p.done:
				return
			case <-wake:
			}
			for {
				select {
				case <-p.done:
					return
				default:
				}
				notificationMu.Lock()
				if len(notifications) == 0 {
					notificationMu.Unlock()
					break
				}
				m := notifications[0]
				notifications[0] = uiPacket{}
				notifications = notifications[1:]
				notificationMu.Unlock()
				if _, err := p.handle(m); err != nil {
					log.Printf("UI lifecycle event failed operation=%q error=%v", m.Operation, err)
				}
			}
		}
	}()
	decoder := json.NewDecoder(p.reader)
	for {
		var m uiPacket
		if err := decoder.Decode(&m); err != nil {
			return
		}
		if m.Reply {
			p.mu.Lock()
			ch := p.pending[m.ID]
			p.mu.Unlock()
			if ch != nil {
				ch <- m
			}
			continue
		}
		if m.ID == 0 {
			notificationMu.Lock()
			notifications = append(notifications, m)
			notificationMu.Unlock()
			select {
			case wake <- struct{}{}:
			default:
			}
			continue
		}
		// A request may itself need a reply from the other process. Keep decoding.
		go func(m uiPacket) {
			value, err := p.handle(m)
			reply := uiPacket{ID: m.ID, Reply: true}
			if err != nil {
				reply.Error = hostapi.Marshal(err)
				if len(reply.Error) == 0 {
					reply.Error, _ = json.Marshal(err.Error())
				}
			}
			if value != nil {
				var marshalErr error
				reply.Data, marshalErr = json.Marshal(value)
				if marshalErr != nil {
					reply.Error = hostapi.Marshal(marshalErr)
				}
			}
			_ = p.send(reply)
		}(m)
	}
}
func (p *uiPeer) call(ctx context.Context, operation string, data any, out any) error {
	payload, err := json.Marshal(data)
	if err != nil {
		return err
	}
	id := p.next.Add(1)
	ch := make(chan uiPacket, 1)
	p.mu.Lock()
	p.pending[id] = ch
	p.mu.Unlock()
	defer func() { p.mu.Lock(); delete(p.pending, id); p.mu.Unlock() }()
	if err := p.send(uiPacket{ID: id, Operation: operation, Data: payload}); err != nil {
		return err
	}
	select {
	case reply := <-ch:
		if len(reply.Error) > 0 {
			var wire struct {
				Code    hostapi.CommandCode `json:"code"`
				Message string              `json:"message"`
			}
			if json.Unmarshal(reply.Error, &wire) == nil && wire.Code != "" {
				return hostapi.New(wire.Code, wire.Message)
			}
			var message string
			if json.Unmarshal(reply.Error, &message) == nil {
				return hostapi.TextError(message)
			}
			return errors.New("resident host command failed")
		}
		if out != nil && len(reply.Data) > 0 {
			return json.Unmarshal(reply.Data, out)
		}
		return nil
	case <-p.done:
		return errors.New("UI lifecycle connection closed")
	case <-ctx.Done():
		return ctx.Err()
	}
}
func (p *uiPeer) close() { _ = p.writer.Close(); _ = p.reader.Close() }
