package daemonclient

import (
	"context"
	"encoding/json"
	"fmt"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/errctx"
	"net/http"

	"github.com/coder/websocket"
)

// Event is one daemon WebSocket event `{ "type": ..., "payload": ... }`.
type Event struct {
	Type    string          `json:"type"`
	Payload json.RawMessage `json:"payload"`
}

// WS is an authenticated daemon WebSocket connection.
type WS struct {
	conn *websocket.Conn
}

// DialWS opens an authenticated WebSocket. The daemon grants a control lease
// for as long as an authenticated connection stays open.
func (c *Client) DialWS(ctx context.Context) (*WS, error) {
	token, err := c.SessionToken(ctx)
	if err != nil {
		return nil, errctx.Wrap("failed to exchange session token for WS", err)
	}
	header := http.Header{}
	header.Set("Authorization", "Session "+token)
	conn, resp, err := websocket.Dial(ctx, c.WSURL, &websocket.DialOptions{HTTPHeader: header, HTTPClient: NewLocalHTTPClient(0)})
	if err != nil {
		if resp != nil && resp.StatusCode != http.StatusSwitchingProtocols {
			// tungstenite's `Error::Http` Display, e.g. while a restarted
			// daemon is not ready yet ("HTTP error: 503 Service Unavailable").
			return nil, fmt.Errorf("WS handshake failed: HTTP error: %s", StatusText(resp.StatusCode))
		}
		return nil, fmt.Errorf("WS handshake failed: %w", err)
	}
	conn.SetReadLimit(-1)
	return &WS{conn: conn}, nil
}

// Subscribe sends the topic subscription request.
func (w *WS) Subscribe(ctx context.Context, topics ...string) error {
	data, _ := json.Marshal(map[string]any{"action": "subscribe", "topics": topics})
	if err := w.conn.Write(ctx, websocket.MessageText, data); err != nil {
		return errctx.Wrap("failed to send WS subscribe", err)
	}
	return nil
}

// Events delivers decoded text events until the connection or ctx ends.
// Non-JSON frames are skipped, as in the Rust client.
func (w *WS) Events(ctx context.Context) <-chan Event {
	out := make(chan Event, 64)
	go func() {
		defer close(out)
		for {
			kind, data, err := w.conn.Read(ctx)
			if err != nil {
				return
			}
			if kind != websocket.MessageText {
				continue
			}
			var event Event
			if json.Unmarshal(data, &event) != nil {
				continue
			}
			select {
			case out <- event:
			case <-ctx.Done():
				return
			}
		}
	}()
	return out
}

// Close ends the connection with a normal close frame.
func (w *WS) Close() { w.conn.Close(websocket.StatusNormalClosure, "") }

// Lease holds the daemon control lease until Release.
type Lease struct {
	ws     *WS
	cancel context.CancelFunc
}

// HoldLease opens a bare authenticated WebSocket and keeps reading it so
// pings are answered; the daemon treats the connection as a control lease.
func (c *Client) HoldLease(ctx context.Context) (*Lease, error) {
	ws, err := c.DialWS(ctx)
	if err != nil {
		return nil, err
	}
	readCtx, cancel := context.WithCancel(context.Background())
	go func() {
		for {
			if _, _, err := ws.conn.Read(readCtx); err != nil {
				return
			}
		}
	}()
	return &Lease{ws: ws, cancel: cancel}, nil
}

// Release drops the lease.
func (l *Lease) Release() {
	if l == nil {
		return
	}
	l.cancel()
	l.ws.Close()
}
