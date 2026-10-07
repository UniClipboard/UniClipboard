//go:build e2e

package main

import (
	"context"
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

// Frames and animation starts are kept in memory and read back through the `tray-icon-frames` control: a write to the evidence file
// syncs to disk, which would delay the very animation being measured.
var (
	trayIconRecMu sync.Mutex
	trayIconRec   []map[string]any
)

func trayIconRecord(rec map[string]any) {
	trayIconRecMu.Lock()
	trayIconRec = append(trayIconRec, rec)
	trayIconRecMu.Unlock()
}

// e2eTrayIconFrame records every image the tray icon hands to the platform, with its time, so an animation can be checked against the
// design's keyframes and the menu's publish records can be placed beside it.
func e2eTrayIconFrame(v iconView) {
	trayIconRecord(map[string]any{"kind": "frame", "base": v.base.String(), "dot": v.dot, "left": v.pose.left, "right": v.pose.right, "ns": time.Now().UnixNano()})
}

// e2eTrayIconAnimation records the moment an animation's clock starts.
func e2eTrayIconAnimation(kind animKind, at time.Time) {
	trayIconRecord(map[string]any{"kind": "animation", "animation": int(kind), "ns": at.UnixNano()})
}

// e2eTrayIconDeliveryRead records each delivery view read of the feed.
func e2eTrayIconDeliveryRead(entryID, targetID string) {
	trayIconRecord(map[string]any{"kind": "delivery-read", "entry": entryID, "target": targetID, "ns": time.Now().UnixNano()})
}

// controlTrayIcon handles the tray icon controls. Everything below that sets a state or sends an event is MANUAL control for visual
// coverage and is recorded as such; the authoritative path is the feed reading the real daemon, which the pause scenario uses.
func (s *EvidenceService) controlTrayIcon(action string) (bool, error) {
	if !strings.HasPrefix(action, "tray-icon") {
		return false, nil
	}
	t := s.host.tray
	if t == nil || t.icon == nil || t.feed == nil {
		return true, s.write(Step{Window: "tray", Step: action, OK: false, Detail: "the tray is not created yet"})
	}
	icon, feed := t.icon, t.feed
	switch {
	case action == "tray-icon-state":
		icon.mu.Lock()
		detail := map[string]any{"base": icon.facts.base().String(), "dot": icon.facts.newContent, "facts": icon.facts,
			"left": icon.pose.left, "right": icon.pose.right, "animating": icon.play != nil, "timer": icon.timer != nil, "cachedFrames": len(icon.frames),
			"reducedMotion": icon.reduced()}
		icon.mu.Unlock()
		feed.deliveryMu.Lock()
		detail["deliveryUnresolved"], detail["deliveryDue"] = len(feed.deliveryUnresolved), len(feed.deliveryDue)
		feed.deliveryMu.Unlock()
		return true, s.write(Step{Window: "tray", Step: "tray-icon-state", OK: true, Detail: detail})
	case strings.HasPrefix(action, "tray-icon-manual:"):
		// tray-icon-manual:<state>[+dot]: MANUAL facts, not daemon state. "none" clears every manual fact.
		spec := strings.TrimPrefix(action, "tray-icon-manual:")
		dot := strings.HasSuffix(spec, "+dot")
		name := strings.TrimSuffix(spec, "+dot")
		icon.update(func(f *iconFacts) {
			f.decisionPending, f.sendFailed, f.locked, f.offline, f.notRecording, f.transferring, f.lanOnly, f.syncPaused, f.newContent = false, false, false, false, false, false, false, false, dot
			switch name {
			case "attention":
				f.decisionPending = true
			case "locked":
				f.locked = true
			case "paused":
				f.syncPaused = true
			case "offline":
				f.offline = true
			case "not-recording":
				f.notRecording = true
			case "transferring":
				f.transferring = true
			case "lan-only":
				f.lanOnly = true
			}
		})
		return true, s.write(Step{Window: "tray", Step: "tray-icon-manual", OK: true, Detail: map[string]any{"state": spec, "manual": true}})
	case strings.HasPrefix(action, "tray-icon-play:"):
		// tray-icon-play:<new|sent|attention|transferring>: MANUAL animation trigger through the same animate() the feed calls.
		kind := map[string]animKind{"new": animNewContent, "sent": animSent, "attention": animAttention, "transferring": animTransferring}[strings.TrimPrefix(action, "tray-icon-play:")]
		icon.animate(kind)
		return true, s.write(Step{Window: "tray", Step: "tray-icon-play", OK: kind != animNone, Detail: map[string]any{"action": action, "manual": true}})
	case strings.HasPrefix(action, "tray-icon-event:"):
		// tray-icon-event:<daemon event type>: a MANUAL synthetic daemon event through the feed's own handler.
		payloads := map[string]string{
			"clipboard.new_content":        `{"origin":"remote"}`,
			"file-transfer.progress":       `{"transferId":"e2e-manual"}`,
			"file-transfer.status_changed": `{"transferId":"e2e-manual","status":"completed"}`,
			// The read of this entry's delivery view goes to the real daemon, which has no such entry, so it fails (F7).
			"clipboard.delivery_status_changed": `{"entryId":"e2e-missing-entry","targetDeviceId":"e2e-target"}`,
		}
		typ := strings.TrimPrefix(action, "tray-icon-event:")
		payload, ok := payloads[typ]
		if ok {
			feed.handle(context.Background(), daemonEvent(typ, payload))
		}
		return true, s.write(Step{Window: "tray", Step: "tray-icon-event", OK: ok, Detail: map[string]any{"type": typ, "manual": true}})
	case strings.HasPrefix(action, "tray-icon-compare:"):
		// tray-icon-compare:<reference dir>: the renderer's macOS-light frames against the design boards rasterized independently, and the
		// template alpha rules (eyes and badge ring are transparent knock-outs).
		detail, ok := compareTrayIconToDesign(strings.TrimPrefix(action, "tray-icon-compare:"))
		return true, s.write(Step{Window: "tray", Step: "tray-icon-compare", OK: ok, Detail: detail})
	case strings.HasPrefix(action, "tray-icon-frames:"):
		// tray-icon-frames:<sinceNs>: the frame and animation records from that time on.
		since, _ := strconv.ParseInt(strings.TrimPrefix(action, "tray-icon-frames:"), 10, 64)
		trayIconRecMu.Lock()
		var recs []map[string]any
		for _, r := range trayIconRec {
			if r["ns"].(int64) >= since {
				recs = append(recs, r)
			}
		}
		trayIconRecMu.Unlock()
		return true, s.write(Step{Window: "tray", Step: "tray-icon-frames", OK: true, Detail: recs})
	case action == "tray-icon-look":
		feed.userLooked()
		return true, s.write(Step{Window: "tray", Step: "tray-icon-look", OK: true})
	case strings.HasPrefix(action, "tray-icon-export:"):
		// tray-icon-export:<dir>: every state, with and without the dot, as the platform's own frame and as the design's macOS-light
		// reference palette (for comparison with the design boards rasterized independently).
		dir := strings.TrimPrefix(action, "tray-icon-export:")
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return true, s.write(Step{Window: "tray", Step: "tray-icon-export", OK: false, Detail: err.Error()})
		}
		count := 0
		for b := baseSynced; b <= baseAttention; b++ {
			for _, dot := range []bool{false, true} {
				v := iconView{base: b, dot: dot}
				data, err := renderTrayFrame(v)
				if err != nil {
					return true, s.write(Step{Window: "tray", Step: "tray-icon-export", OK: false, Detail: err.Error()})
				}
				name := fmt.Sprintf("%s-dot%v", b, dot)
				_ = os.WriteFile(filepath.Join(dir, name+".platform.png"), data, 0o644)
				ref := renderIcon(iconSpec{base: b, dot: dot, size: 44, art: 36, pal: iconPalette{fg: color.NRGBA{0x1D, 0x1D, 0x1F, 255}}})
				f, err := os.Create(filepath.Join(dir, name+".reference.png"))
				if err == nil {
					_ = png.Encode(f, ref)
					_ = f.Close()
				}
				count++
			}
		}
		return true, s.write(Step{Window: "tray", Step: "tray-icon-export", OK: true, Detail: map[string]any{"dir": dir, "frames": count}})
	}
	return false, nil
}

func daemonEvent(typ, payload string) daemonclient.Event {
	var e daemonclient.Event
	e.Type = typ
	e.Payload = json.RawMessage(payload)
	return e
}

// designStates maps the reference file names to what the renderer draws.
var designStates = []struct {
	name string
	base iconBase
	dot  bool
}{
	{"synced", baseSynced, false}, {"transferring", baseTransferring, false}, {"new", baseSynced, true}, {"paused", basePaused, false},
	{"lan-only", baseLANOnly, false}, {"offline", baseOffline, false}, {"not-recording", baseNotRecording, false},
	{"locked", baseLocked, false}, {"attention", baseAttention, false},
}

// compareTrayIconToDesign renders every state with the design's macOS-light colours on its menu-bar background and counts the pixels that
// differ from the reference raster by more than fuzz (out of 255) in any channel.
func compareTrayIconToDesign(dir string) (map[string]any, bool) {
	const fuzz = 77 // 30%: anti-aliasing differences between two rasterizers
	bg := color.NRGBA{0xE9, 0xE9, 0xEC, 255}
	results := map[string]any{}
	ok := true
	for _, st := range designStates {
		f, err := os.Open(filepath.Join(dir, st.name+".png"))
		if err != nil {
			results[st.name] = err.Error()
			ok = false
			continue
		}
		ref, err := png.Decode(f)
		_ = f.Close()
		if err != nil {
			results[st.name] = err.Error()
			ok = false
			continue
		}
		mine := renderIcon(iconSpec{base: st.base, dot: st.dot, size: 44, art: 36, pal: iconPalette{fg: color.NRGBA{0x1D, 0x1D, 0x1F, 255}}})
		differing, worst := 0, 0
		for y := 0; y < 44; y++ {
			for x := 0; x < 44; x++ {
				c := mine.NRGBAAt(x, y)
				a := int(c.A)
				over := [3]int{(int(c.R)*a + int(bg.R)*(255-a)) / 255, (int(c.G)*a + int(bg.G)*(255-a)) / 255, (int(c.B)*a + int(bg.B)*(255-a)) / 255}
				rr, rg, rb, _ := ref.At(x, y).RGBA()
				d := 0
				for i, rv := range [3]int{int(rr >> 8), int(rg >> 8), int(rb >> 8)} {
					if diff := abs(over[i] - rv); diff > d {
						d = diff
					}
				}
				if d > fuzz {
					differing++
				}
				if d > worst {
					worst = d
				}
			}
		}
		// The tolerance is a small fraction of the 44 x 44 image: two rasterizers disagree only along edges.
		pass := differing <= 30
		results[st.name] = map[string]any{"differingPixels": differing, "worstChannelDiff": worst, "pass": pass}
		ok = ok && pass
	}
	// Template rule: macOS reads alpha only, so the eyes and the badge ring must be transparent in the production frame.
	alphaAt := func(img *image.NRGBA, ux, uy float64) uint8 {
		scale := 36.0 / 24
		return img.NRGBAAt(int(4+ux*scale), int(4+uy*scale)).A
	}
	synced := renderIcon(iconSpec{base: baseSynced, size: 44, art: 36, pal: iconPalette{fg: color.NRGBA{A: 255}}})
	badge := renderIcon(iconSpec{base: baseTransferring, size: 44, art: 36, pal: iconPalette{fg: color.NRGBA{A: 255}}})
	checks := map[string]any{
		"eyeAlpha":      alphaAt(synced, 9, 13.8),
		"faceAlpha":     alphaAt(synced, 12, 17.5),
		"ringAlpha":     alphaAt(badge, 15.3, 15.3),
		"badgeCentreIs": alphaAt(badge, 19, 19),
	}
	templateOK := checks["eyeAlpha"] == uint8(0) && checks["faceAlpha"] == uint8(255) && checks["ringAlpha"] == uint8(0)
	results["templateAlpha"] = map[string]any{"samples": checks, "pass": templateOK}
	return results, ok && templateOK
}

func abs(v int) int {
	if v < 0 {
		return -v
	}
	return v
}
