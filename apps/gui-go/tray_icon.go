package main

import (
	"bytes"
	"image/png"
	"math"
	"sync"
	"time"

	"github.com/wailsapp/wails/v3/pkg/application"
)

// trayIcon owns what the tray icon shows: the "B solid cat" state derived from daemon facts, and the ear animation that plays on
// events (docs/architecture/gui-go-tray-icon.md). It only ever calls the platform's icon API, never the menu's publish path, so a
// frame or a state change cannot close an open menu (t-0188). While nothing animates no timer exists.
//
// Facts and animations only change state under mu and ask for a repaint; one painter goroutine renders and hands the newest state to the
// platform. Wails runs the tray's icon call on the main thread and waits for it, and the tray's click handler runs ON the main thread, so
// no caller may hold mu or be on that thread while it waits: only the painter makes the call.
type trayIcon struct {
	tray *application.SystemTray

	mu     sync.Mutex
	facts  iconFacts
	pose   earPose
	frames map[frameKey][]byte // encoded images by what they show; the set is small and bounded by the keyframes

	play    *animation
	timer   *time.Timer
	gen     uint64 // bumped whenever the timer chain is replaced or stopped, so a late callback does nothing
	closed  bool
	reduced func() bool // the system's "reduce motion" setting, read when an animation would start

	paint chan struct{} // capacity 1: a pending repaint request; later requests merge into it
	done  chan struct{}
	last  frameKey // what the platform last received; only the painter reads and writes it
	shown bool
}

// frameKey identifies an encoded image: what it shows and the platform's own variant (the taskbar theme on Windows, the colour scheme on Linux).
type frameKey struct {
	view    iconView
	variant string
}

// iconFacts are the daemon facts the icon depends on. Each comes from a daemon read or event (tray_icon_facts.go); none is guessed.
type iconFacts struct {
	decisionPending bool // the daemon waits for the user: a device-trust change to decide, or a device asking to pair
	sendFailed      bool // the daemon reports a failed delivery the user has not looked at
	locked          bool // content lock is on
	syncPaused      bool // the user turned sync off
	offline         bool // devices are paired and none is reachable
	notRecording    bool // the foreground app is excluded from history; the daemon exposes no such fact yet, so only the e2e controls set it
	transferring    bool // a transfer has been running for longer than transferringAfter
	lanOnly         bool // relay fallback is off
	newContent      bool // remote content arrived and the user has not looked yet
}

// base is the one state the cat shows. The design does not rank states against each other; this order is a product rule: what
// needs the user first, then what stops sync, then what is only informational.
func (f iconFacts) base() iconBase {
	switch {
	case f.decisionPending || f.sendFailed:
		return baseAttention
	case f.locked:
		return baseLocked
	case f.syncPaused:
		return basePaused
	case f.offline:
		return baseOffline
	case f.notRecording:
		return baseNotRecording
	case f.transferring:
		return baseTransferring
	case f.lanOnly:
		return baseLANOnly
	}
	return baseSynced
}

// earPose is how far each ear is turned, in degrees (the left ear turns outward with a negative angle).
type earPose struct{ left, right float64 }

// iconView is everything that decides the pixels, apart from the platform's own size and colours.
type iconView struct {
	base iconBase
	dot  bool
	pose earPose
}

func newTrayIcon(tray *application.SystemTray) *trayIcon {
	prepareTrayPlatform()
	return &trayIcon{tray: tray, frames: map[frameKey][]byte{}, reduced: systemReducesMotion, paint: make(chan struct{}, 1), done: make(chan struct{})}
}

// start puts the first frame on the tray; the facts are still the defaults until the daemon answers.
func (i *trayIcon) start() {
	go i.painter()
	i.requestPaint()
}

func (i *trayIcon) view() iconView {
	return iconView{base: i.facts.base(), dot: i.facts.newContent, pose: i.pose}
}

// requestPaint asks the painter to show the current state. It never blocks.
func (i *trayIcon) requestPaint() {
	select {
	case i.paint <- struct{}{}:
	default: // one is already pending and will read the latest state
	}
}

// update changes the facts under the lock and asks for a repaint. Which animation (if any) follows is decided by the caller from the
// change it made.
func (i *trayIcon) update(change func(*iconFacts)) (before, after iconFacts) {
	i.mu.Lock()
	before = i.facts
	change(&i.facts)
	after = i.facts
	i.mu.Unlock()
	i.requestPaint()
	return before, after
}

// painter is the only goroutine that calls the platform: it shows the newest state and skips what is already on the tray.
func (i *trayIcon) painter() {
	for {
		select {
		case <-i.paint:
		case <-i.done:
			return
		}
		i.mu.Lock()
		v := i.view()
		key := frameKey{view: v, variant: trayFrameVariant()}
		if i.closed || (i.shown && i.last == key) {
			i.mu.Unlock()
			continue
		}
		data, ok := i.frames[key]
		if !ok {
			var err error
			if data, err = renderTrayFrame(v); err != nil {
				i.mu.Unlock()
				continue
			}
			i.frames[key] = data
		}
		i.last, i.shown = key, true
		i.mu.Unlock()
		applyTrayIcon(i.tray, data)
		e2eTrayIconFrame(v)
	}
}

// encodeIcon renders and PNG-encodes one image.
func encodeIcon(spec iconSpec) ([]byte, error) {
	var buf bytes.Buffer
	if err := png.Encode(&buf, renderIcon(spec)); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}

// animation is a timeline of ear poses between keyframes (milliseconds from the start).
type animation struct {
	kind    animKind
	frames  []keyframe
	loop    time.Duration // 0: play once; otherwise the timeline repeats every loop
	limit   time.Duration // 0: until the timeline ends; otherwise stop after this long (a looping timeline)
	started time.Time
}

type keyframe struct {
	at          time.Duration
	left, right float64
}

type animKind int

// Higher wins: a new animation interrupts the one playing unless that one ranks higher.
const (
	animNone animKind = iota
	animTransferring
	animSent
	animNewContent
	animAttention
)

// frameStep is how often a playing animation redraws: the tray gets a new image at most this often, and only while one plays.
const frameStep = 40 * time.Millisecond

func ms(n int) time.Duration { return time.Duration(n) * time.Millisecond }

// The timelines are the keyframe strips of the "TrayBMotion" board. Between keyframes the pose is interpolated linearly; the
// design's CSS demo uses cubic-bezier easing and slightly different percentages, but states the product timing in text and strip.
var (
	// 520 ms, once: the right ear flicks out 18 degrees, then 12.
	animNewContentFrames = []keyframe{{ms(0), 0, 0}, {ms(110), 0, 18}, {ms(250), 0, 0}, {ms(370), 0, 12}, {ms(520), 0, 0}}
	// 360 ms, once: both ears swing outward 14 degrees.
	animSentFrames = []keyframe{{ms(0), 0, 0}, {ms(190), -14, 14}, {ms(360), 0, 0}}
	// 1.6 s per round, at most 3 s: the ears tap in turn, 9 degrees.
	animTransferringFrames = []keyframe{{ms(0), 0, 0}, {ms(400), -9, 0}, {ms(800), 0, 0}, {ms(1200), 0, 9}, {ms(1600), 0, 0}}
	// 700 ms, once: both ears shake three times, 15 degrees. The board strip shows two peaks and states 700 ms; the demo repeats
	// every 280 ms with the third peak at 672 ms, so the same period is kept and the last return lands at 700 ms.
	animAttentionFrames = []keyframe{{ms(0), 0, 0}, {ms(110), -15, 15}, {ms(250), 0, 0}, {ms(390), -15, 15}, {ms(530), 0, 0}, {ms(670), -15, 15}, {ms(700), 0, 0}}
)

const (
	transferringAnimationLimit = 3 * time.Second
	transferringLoop           = 1600 * time.Millisecond
)

func animationFor(kind animKind) *animation {
	switch kind {
	case animNewContent:
		return &animation{kind: kind, frames: animNewContentFrames}
	case animSent:
		return &animation{kind: kind, frames: animSentFrames}
	case animAttention:
		return &animation{kind: kind, frames: animAttentionFrames}
	case animTransferring:
		return &animation{kind: kind, frames: animTransferringFrames, loop: transferringLoop, limit: transferringAnimationLimit}
	}
	return nil
}

// poseAt interpolates the timeline; done reports that the animation is over.
func (a *animation) poseAt(elapsed time.Duration) (p earPose, done bool) {
	if a.limit > 0 && elapsed >= a.limit {
		return earPose{}, true
	}
	if a.loop > 0 {
		elapsed %= a.loop
	} else if elapsed >= a.frames[len(a.frames)-1].at {
		return earPose{}, true
	}
	for n := 1; n < len(a.frames); n++ {
		from, to := a.frames[n-1], a.frames[n]
		if elapsed <= to.at {
			span := float64(to.at - from.at)
			t := 0.0
			if span > 0 {
				t = float64(elapsed-from.at) / span
			}
			return earPose{from.left + (to.left-from.left)*t, from.right + (to.right-from.right)*t}, false
		}
	}
	return earPose{}, true
}

// quantize rounds the angles so the set of distinct frames, and so the encoded image cache, stays small.
func (p earPose) quantize() earPose {
	return earPose{math.Round(p.left), math.Round(p.right)}
}

// animate starts an animation unless the system reduces motion or a higher-ranked one is playing.
func (i *trayIcon) animate(kind animKind) {
	if i.reduced() { // read before the lock: it may ask the desktop (Linux)
		return // with reduced motion the state still changes, only the ears do not move
	}
	i.mu.Lock()
	defer i.mu.Unlock()
	if i.closed || (i.play != nil && i.play.kind > kind) {
		return
	}
	a := animationFor(kind)
	if a == nil {
		return
	}
	i.stopLocked()
	a.started = time.Now()
	i.play = a
	e2eTrayIconAnimation(kind, a.started)
	i.stepLocked()
}

// stopAnimation ends the animation of the given kind (a looping one whose cause is over); other kinds keep playing.
func (i *trayIcon) stopAnimation(kind animKind) {
	i.mu.Lock()
	defer i.mu.Unlock()
	if i.play != nil && i.play.kind == kind {
		i.endLocked()
	}
}

func (i *trayIcon) stopLocked() {
	i.gen++
	if i.timer != nil {
		i.timer.Stop()
		i.timer = nil
	}
}

// endLocked stops playing and puts the ears back.
func (i *trayIcon) endLocked() {
	i.stopLocked()
	i.play = nil
	i.pose = earPose{}
	i.requestPaint()
}

// stepLocked draws the pose for now and schedules the next frame, or ends the animation.
func (i *trayIcon) stepLocked() {
	if i.play == nil {
		return
	}
	pose, done := i.play.poseAt(time.Since(i.play.started))
	if done {
		i.endLocked()
		return
	}
	i.pose = pose.quantize()
	i.requestPaint()
	gen := i.gen
	// The next frame is due on the start's step grid, not frameStep after this one finished, so drawing time does not stretch the animation.
	elapsed := time.Since(i.play.started)
	next := (elapsed/frameStep + 1) * frameStep
	i.timer = time.AfterFunc(next-elapsed, func() {
		i.mu.Lock()
		defer i.mu.Unlock()
		if gen != i.gen || i.closed {
			return
		}
		i.stepLocked()
	})
}

// close stops everything; the tray is going away.
func (i *trayIcon) close() {
	i.mu.Lock()
	defer i.mu.Unlock()
	if i.closed {
		return
	}
	i.closed = true
	i.stopLocked()
	i.play = nil
	close(i.done)
}
