package scenarios

import (
	"context"
	"encoding/json"
	"fmt"
	"net"
	"strings"
	"time"
)

type automationClient struct {
	socket string
}

type autoWindow struct {
	ID      int    `json:"id"`
	Title   string `json:"title"`
	Kind    string `json:"kind"`
	Focused bool   `json:"focused"`
	Visible bool   `json:"visible"`
	Bounds  struct {
		X      float64 `json:"x"`
		Y      float64 `json:"y"`
		Width  float64 `json:"width"`
		Height float64 `json:"height"`
	} `json:"bounds"`
}

type autoNode struct {
	ID     string          `json:"id"`
	Role   string          `json:"role"`
	Label  string          `json:"label"`
	Value  string          `json:"value"`
	Key    json.RawMessage `json:"key"`
	States []string        `json:"states"`
	Bounds struct {
		X      float64 `json:"x"`
		Y      float64 `json:"y"`
		Width  float64 `json:"width"`
		Height float64 `json:"height"`
	} `json:"bounds"`
}

func (n autoNode) KeyString() string {
	if len(n.Key) == 0 {
		return ""
	}
	var s string
	if json.Unmarshal(n.Key, &s) == nil {
		return s
	}
	return strings.TrimSpace(string(n.Key))
}

func (c *automationClient) call(ctx context.Context, method string, params any, out any) error {
	deadline := 10 * time.Second
	conn, err := (&net.Dialer{Timeout: 2 * time.Second}).DialContext(ctx, "unix", c.socket)
	if err != nil {
		return err
	}
	defer func() { _ = conn.Close() }()
	if err := conn.SetDeadline(time.Now().Add(deadline)); err != nil {
		return err
	}
	if err := json.NewEncoder(conn).Encode(map[string]any{
		"id":     1,
		"method": method,
		"params": params,
	}); err != nil {
		return err
	}
	var resp struct {
		Result json.RawMessage `json:"result"`
		Error  *struct {
			Code    string `json:"code"`
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := json.NewDecoder(conn).Decode(&resp); err != nil {
		return err
	}
	if resp.Error != nil {
		return fmt.Errorf("automation %s (%s): %s", method, resp.Error.Code, resp.Error.Message)
	}
	if out != nil && len(resp.Result) > 0 {
		return json.Unmarshal(resp.Result, out)
	}
	return nil
}

func (c *automationClient) Windows(ctx context.Context) ([]autoWindow, error) {
	var res struct {
		Windows []autoWindow `json:"windows"`
	}
	err := c.call(ctx, "windows", nil, &res)
	return res.Windows, err
}

func (c *automationClient) Tree(ctx context.Context, winID int) ([]autoNode, error) {
	var res struct {
		Nodes []autoNode `json:"nodes"`
	}
	err := c.call(ctx, "tree", map[string]any{"window": winID}, &res)
	return res.Nodes, err
}

func (c *automationClient) Idle(ctx context.Context, winID int, timeoutMs int) error {
	return c.call(ctx, "idle", map[string]any{"window": winID, "timeout": timeoutMs}, nil)
}

func (c *automationClient) Act(ctx context.Context, winID int, nodeID, action, text string) error {
	params := map[string]any{
		"window": winID,
		"id":     nodeID,
		"action": action,
	}
	if text != "" || action == "setValue" {
		params["text"] = text
	}
	return c.call(ctx, "act", params, nil)
}

func (c *automationClient) Input(ctx context.Context, winID int, events []map[string]any) error {
	return c.call(ctx, "input", map[string]any{"window": winID, "events": events}, nil)
}

func (c *automationClient) WaitContentWindow(ctx context.Context, excludePrefix string) (autoWindow, error) {
	wctx, cancel := context.WithTimeout(ctx, 20*time.Second)
	defer cancel()
	for {
		ws, err := c.Windows(wctx)
		if err == nil {
			for _, w := range ws {
				if w.Visible && w.Kind == "content" && (excludePrefix == "" || !strings.HasPrefix(w.Title, excludePrefix)) {
					return w, nil
				}
			}
		}
		select {
		case <-wctx.Done():
			return autoWindow{}, fmt.Errorf("wait content window on %s: %w", c.socket, wctx.Err())
		case <-time.After(100 * time.Millisecond):
		}
	}
}

func (c *automationClient) WaitSearchReady(ctx context.Context, excludePrefix string) (int, error) {
	wctx, cancel := context.WithTimeout(ctx, 25*time.Second)
	defer cancel()
	for {
		w, err := c.WaitContentWindow(wctx, excludePrefix)
		if err == nil {
			nodes, terr := c.Tree(wctx, w.ID)
			if terr == nil {
				for _, n := range nodes {
					if n.Role == "comboBox" || n.Role == "textField" || n.Role == "search" {
						return w.ID, nil
					}
				}
			}
		}
		select {
		case <-wctx.Done():
			return 0, fmt.Errorf("wait search control ready on %s: %w", c.socket, wctx.Err())
		case <-time.After(100 * time.Millisecond):
		}
	}
}

func findNodeByRoleAndLabels(nodes []autoNode, role string, labels ...string) (autoNode, bool) {
	for _, n := range nodes {
		if role != "" && n.Role != role {
			continue
		}
		for _, want := range labels {
			if n.Label == want {
				return n, true
			}
		}
	}
	return autoNode{}, false
}

func findNodeByKey(nodes []autoNode, role, key string) (autoNode, bool) {
	for _, n := range nodes {
		if (role == "" || n.Role == role) && n.KeyString() == key {
			return n, true
		}
	}
	return autoNode{}, false
}
