package e2e

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strconv"
	"strings"
	"time"
)

type detailObservation struct {
	Phase       string   `json:"phase"`
	AtMS        int64    `json:"at_ms"`
	Selected    string   `json:"selected"`
	Immediate   bool     `json:"immediate"`
	BodyPresent bool     `json:"body_present"`
	Loading     []string `json:"loading"`
}

// cycleDetails inspects the actual detail region immediately after input. It
// never waits for a failed click assertion to become ready. The interval before
// cycling allows the initial visible window to prefetch without selection.
func (s *desktopScenario) cycleDetails(args []string) (resultErr error) {
	if len(args) != 3 {
		return errors.New("ui-detail-cycle rounds row-prefix phase")
	}
	rounds, err := strconv.Atoi(args[0])
	if err != nil || rounds < 1 || rounds > 5 {
		return errors.New("rounds must be between one and five")
	}
	select {
	case <-time.After(1200 * time.Millisecond):
	case <-s.ctx.Done():
		return s.ctx.Err()
	}
	nodes, err := s.tree()
	if err != nil {
		return err
	}
	var rows []string
	var list desktopNode
	for _, n := range nodes {
		if n.Role == "list" && n.Label == "历史条目" {
			list = n
		}
	}
	for _, n := range nodes {
		if n.Role != "listItem" || !strings.HasPrefix(n.Label, args[1]) {
			continue
		}
		if slices.Contains(n.States, "offscreen") || n.Bounds.Height <= 0 || n.Bounds.Y < list.Bounds.Y || n.Bounds.Y+n.Bounds.Height > list.Bounds.Y+list.Bounds.Height {
			return fmt.Errorf("target %q is not fully inside the visible list", n.Label)
		}
		rows = append(rows, n.Label)
	}
	if len(rows) != 8 {
		return fmt.Errorf("need eight visible evidence rows; found %d", len(rows))
	}
	if args[2] == "search" {
		active := false
		for _, n := range nodes {
			if n.Role == "comboBox" && n.Label == "搜索历史" && strings.Contains(n.Value, args[1]) {
				active = true
			}
		}
		if !active {
			return errors.New("search phase has no active search query")
		}
	}
	path := filepath.Join(s.out, "detail-observations.json")
	var observations []detailObservation
	if data, err := os.ReadFile(path); err == nil {
		if err := json.Unmarshal(data, &observations); err != nil {
			return err
		}
	} else if !os.IsNotExist(err) {
		return err
	}
	defer func() {
		data, err := json.MarshalIndent(observations, "", "  ")
		if err == nil {
			err = os.WriteFile(path, data, 0600)
		}
		if err != nil {
			resultErr = errors.Join(resultErr, err)
		}
	}()
	var acknowledged []desktopNode
	check := func(want string, immediate bool) error {
		nodes, err := s.tree()
		if immediate && acknowledged != nil {
			nodes, err = acknowledged, nil
			acknowledged = nil
		}
		if err != nil {
			return err
		}
		byID := map[string]desktopNode{}
		detail := ""
		for _, n := range nodes {
			byID[n.ID] = n
			if n.Role == "region" && n.Label == "历史详情" {
				detail = n.ID
			}
		}
		if detail == "" {
			return errors.New("detail region missing")
		}
		observation := detailObservation{Phase: args[2], AtMS: time.Since(s.started).Milliseconds(), Selected: want, Immediate: immediate, Loading: []string{}}
		for _, n := range nodes {
			inside := false
			for parent, depth := n.Parent, 0; parent != "" && depth < len(nodes); depth++ {
				if parent == detail {
					inside = true
					break
				}
				parent = byID[parent].Parent
			}
			if !inside {
				continue
			}
			if n.Role == "text" && n.Label == want {
				observation.BodyPresent = true
			}
			for _, forbidden := range []string{"正在读取", "读取中", "获取中", "正在获取", "加载中", "正在加载"} {
				if strings.Contains(n.Label, forbidden) {
					observation.Loading = append(observation.Loading, n.Label)
					break
				}
			}
		}
		observations = append(observations, observation)
		if !observation.BodyPresent || len(observation.Loading) != 0 {
			return fmt.Errorf("detail after selecting %q: body=%t loading=%v", want, observation.BodyPresent, observation.Loading)
		}
		return nil
	}
	sequence := slices.Clone(rows)
	for i := len(rows) - 2; i > 0; i-- {
		sequence = append(sequence, rows[i])
	}
	clicks := 0
	for round := 0; round < rounds; round++ {
		for _, label := range sequence {
			node, err := s.find("listItem", label)
			if err != nil {
				return err
			}
			if err := s.call(s.ctx, "input", map[string]any{"window": s.window, "events": []any{map[string]any{"type": "click", "x": node.Bounds.X + node.Bounds.Width/2, "y": node.Bounds.Y + node.Bounds.Height/2}}}, nil); err != nil {
				return err
			}
			if err := s.wait(func() bool {
				nodes, err := s.tree()
				if err != nil {
					return false
				}
				for _, n := range nodes {
					if n.Role == "listItem" && n.Label == label && slices.Contains(n.States, "checked") {
						acknowledged = nodes
						return true
					}
				}
				return false
			}); err != nil {
				return fmt.Errorf("selection did not take effect: %w", err)
			}
			if err := check(label, true); err != nil {
				return err
			}
			// Keep each detail readable in the uncut video and check intervening frames.
			timer := time.NewTimer(300 * time.Millisecond)
			ticker := time.NewTicker(25 * time.Millisecond)
		observe:
			for {
				select {
				case <-timer.C:
					break observe
				case <-ticker.C:
					if err := check(label, false); err != nil {
						timer.Stop()
						ticker.Stop()
						return err
					}
				case <-s.ctx.Done():
					timer.Stop()
					ticker.Stop()
					return s.ctx.Err()
				}
			}
			ticker.Stop()
			clicks++
		}
	}
	s.result.Assertions = append(s.result.Assertions, fmt.Sprintf("%s: %d visible-row clicks in both directions; exact detail body immediately present and no loading text in any observation", args[2], clicks))
	return nil
}
