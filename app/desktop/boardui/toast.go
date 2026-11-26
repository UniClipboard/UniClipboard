package boardui

import (
	"fmt"
	"slices"
	"strconv"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
)

// NotificationStatus selects the semantic icon and badge colors of a Toast /
// Notification card, derived from BoardUI Notification and Toast.
type NotificationStatus string

const (
	StatusNeutral     NotificationStatus = "neutral"
	StatusInformation NotificationStatus = "information"
	StatusSuccess     NotificationStatus = "success"
	StatusWarning     NotificationStatus = "warning"
	StatusError       NotificationStatus = "error"
)

// NotificationPosition anchors a Toaster stack to a viewport edge or corner.
type NotificationPosition string

const (
	PositionTopStart     NotificationPosition = "top-start"
	PositionTopCenter    NotificationPosition = "top-center"
	PositionTopEnd       NotificationPosition = "top-end"
	PositionBottomStart  NotificationPosition = "bottom-start"
	PositionBottomCenter NotificationPosition = "bottom-center"
	PositionBottomEnd    NotificationPosition = "bottom-end"
)

const (
	// DefaultToastDuration matches BoardUI's Toaster default (5000 ms).
	DefaultToastDuration = 5 * time.Second
	// DefaultToastLimit matches BoardUI's Toaster default (3 simultaneous toasts).
	DefaultToastLimit = 3
	// ToastPersistent keeps a toast on screen until dismissed or updated.
	ToastPersistent = -1 * time.Millisecond
)

// ToastAction describes a small BoardUI Button rendered beneath a toast message.
type ToastAction struct {
	Label string
	// Variant defaults to "secondary" for the first action and "primary" for subsequent actions.
	Variant  string
	Disabled bool
	Busy     bool
	// KeepOpen prevents clicking the action from automatically dismissing the toast.
	KeepOpen bool
	OnClick  func()
}

// ToastOptions configures a toast created through ToastStore.
type ToastOptions struct {
	// ID names the toast. Reusing an active ID replaces that toast in place and restarts its countdown.
	ID          string
	Description string
	Timestamp   string
	Status      NotificationStatus
	// Loading replaces the leading status glyph with a spinner tinted to the status color.
	Loading bool
	// Progress, when non-nil, displays the 3px bottom progress bar at [0..1]
	// instead of a countdown timer bar (typically paired with ToastPersistent).
	Progress *float32
	// CloseLabel overrides ToasterProps.CloseLabel for this toast's close button.
	CloseLabel string
	Actions    []ToastAction
	// Duration overrides the Toaster default. Zero uses ToasterProps.Duration (default 5s);
	// a negative duration (ToastPersistent) disables auto-dismiss.
	Duration time.Duration
	// NonDismissible hides the close button. False (default) shows the close button.
	NonDismissible bool
	// OnDismiss runs once the toast is removed from the stack.
	OnDismiss func()
}

type toastRecord struct {
	id             string
	revision       uint64
	title          string
	description    string
	timestamp      string
	status         NotificationStatus
	loading        bool
	progress       *float32
	closeLabel     string
	actions        []ToastAction
	duration       time.Duration
	nonDismissible bool
	onDismiss      func()

	// Runtime countdown state owned by Toaster during view frames.
	resolvedDuration time.Duration
	remaining        time.Duration
	lastTick         time.Time
}

func cloneFloat32Ptr(v *float32) *float32 {
	if v == nil {
		return nil
	}
	c := *v
	return &c
}

func float32PtrEqual(a, b *float32) bool {
	if a == nil || b == nil {
		return a == b
	}
	return *a == *b
}

// ToastStore holds caller-owned toast items for one Toaster region.
// All methods must be called on the view thread.
type ToastStore struct {
	items  []toastRecord
	nextID uint64
	rev    uint64
	paused bool
}

// Show displays a toast or replaces an existing toast with the same ID.
func (s *ToastStore) Show(title string, opts ToastOptions) string {
	if s == nil {
		return ""
	}
	id := opts.ID
	if id == "" {
		s.nextID++
		id = "toast-" + strconv.FormatUint(s.nextID, 10)
	}
	status := opts.Status
	if status == "" {
		status = StatusNeutral
	}
	s.rev++
	rec := toastRecord{
		id:             id,
		revision:       s.rev,
		title:          title,
		description:    opts.Description,
		timestamp:      opts.Timestamp,
		status:         status,
		loading:        opts.Loading,
		progress:       cloneFloat32Ptr(opts.Progress),
		closeLabel:     opts.CloseLabel,
		actions:        slices.Clone(opts.Actions),
		duration:       opts.Duration,
		nonDismissible: opts.NonDismissible,
		onDismiss:      opts.OnDismiss,
	}
	for i := range s.items {
		if s.items[i].id == id {
			s.items[i] = rec
			return id
		}
	}
	s.items = append(s.items, rec)
	return id
}

// Info shows an informational toast.
func (s *ToastStore) Info(title string, opts ...ToastOptions) string {
	o := firstToastOptions(opts)
	o.Status = StatusInformation
	return s.Show(title, o)
}

// Success shows a success toast.
func (s *ToastStore) Success(title string, opts ...ToastOptions) string {
	o := firstToastOptions(opts)
	o.Status = StatusSuccess
	return s.Show(title, o)
}

// Warning shows a warning toast.
func (s *ToastStore) Warning(title string, opts ...ToastOptions) string {
	o := firstToastOptions(opts)
	o.Status = StatusWarning
	return s.Show(title, o)
}

// Error shows an error toast.
func (s *ToastStore) Error(title string, opts ...ToastOptions) string {
	o := firstToastOptions(opts)
	o.Status = StatusError
	return s.Show(title, o)
}

// Update modifies an existing toast in place. An empty title keeps the existing
// title. Setting a new duration restarts its countdown.
func (s *ToastStore) Update(id, title string, opts ToastOptions) {
	if s == nil || id == "" {
		return
	}
	idx := s.indexOf(id)
	if idx < 0 {
		return
	}
	cur := s.items[idx]
	if title == "" {
		title = cur.title
	}
	if opts.Status == "" {
		opts.Status = cur.status
	}
	if opts.Duration == 0 {
		opts.Duration = cur.duration
	}
	if opts.OnDismiss == nil {
		opts.OnDismiss = cur.onDismiss
	}
	opts.ID = id
	s.Show(title, opts)
}

// SyncPersistent ensures a persistent toast with id matches title/description/
// status/actions without restarting animations or countdowns when unchanged.
func (s *ToastStore) SyncPersistent(id, title string, opts ToastOptions) {
	if s == nil || id == "" {
		return
	}
	opts.ID = id
	opts.Duration = ToastPersistent
	status := opts.Status
	if status == "" {
		status = StatusNeutral
	}
	if idx := s.indexOf(id); idx >= 0 {
		cur := &s.items[idx]
		if cur.title == title && cur.description == opts.Description && cur.timestamp == opts.Timestamp &&
			cur.status == status && cur.loading == opts.Loading && cur.nonDismissible == opts.NonDismissible &&
			float32PtrEqual(cur.progress, opts.Progress) && cur.closeLabel == opts.CloseLabel {
			cur.actions = slices.Clone(opts.Actions)
			cur.onDismiss = opts.OnDismiss
			return
		}
		// Keep revision stable when mutating a persistent toast in place so
		// action buttons do not flicker or lose focus across updates.
		cur.title = title
		cur.description = opts.Description
		cur.timestamp = opts.Timestamp
		cur.status = status
		cur.loading = opts.Loading
		cur.progress = cloneFloat32Ptr(opts.Progress)
		cur.closeLabel = opts.CloseLabel
		cur.actions = slices.Clone(opts.Actions)
		cur.nonDismissible = opts.NonDismissible
		cur.onDismiss = opts.OnDismiss
		cur.duration = ToastPersistent
		cur.resolvedDuration = 0
		cur.remaining = 0
		return
	}
	s.Show(title, opts)
}

// Dismiss removes one toast by ID and runs its OnDismiss callback.
func (s *ToastStore) Dismiss(id string) {
	if s == nil || id == "" {
		return
	}
	idx := s.indexOf(id)
	if idx < 0 {
		return
	}
	cb := s.items[idx].onDismiss
	s.items = slices.Delete(s.items, idx, idx+1)
	if cb != nil {
		cb()
	}
}

// DismissAll removes every active toast and runs their OnDismiss callbacks.
func (s *ToastStore) DismissAll() {
	if s == nil || len(s.items) == 0 {
		return
	}
	prev := s.items
	s.items = nil
	s.paused = false
	for _, rec := range prev {
		if rec.onDismiss != nil {
			rec.onDismiss()
		}
	}
}

// Has reports whether an active toast with id is currently in the store.
func (s *ToastStore) Has(id string) bool {
	return s != nil && s.indexOf(id) >= 0
}

// Len returns the number of active toasts.
func (s *ToastStore) Len() int {
	if s == nil {
		return 0
	}
	return len(s.items)
}

func (s *ToastStore) indexOf(id string) int {
	for i := range s.items {
		if s.items[i].id == id {
			return i
		}
	}
	return -1
}

func firstToastOptions(opts []ToastOptions) ToastOptions {
	if len(opts) > 0 {
		return opts[0]
	}
	return ToastOptions{}
}

// ToasterProps configures the floating BoardUI Toast viewport.
type ToasterProps struct {
	// Position anchors the stack; defaults to PositionBottomEnd.
	Position NotificationPosition
	// Limit bounds simultaneous visible toasts; defaults to DefaultToastLimit (3).
	Limit int
	// Duration is the default auto-dismiss duration; defaults to DefaultToastDuration (5s).
	Duration time.Duration
	// Label is the accessible name of the notifications region.
	Label string
	// CloseLabel is the accessible name of each toast's dismiss button.
	CloseLabel string
}

// NotificationProps renders one standalone BoardUI Notification card.
type NotificationProps struct {
	Title          string
	Description    string
	Timestamp      string
	Status         NotificationStatus
	Loading        bool
	Actions        []ToastAction
	NonDismissible bool
	CloseLabel     string
	// Progress renders the 3px bottom countdown bar when > 0 (range 0..1).
	Progress float32
	// ShowBar forces the countdown bar track when a timer is active.
	ShowBar bool
}

// NotificationResult reports interactions on a standalone Notification card.
type NotificationResult struct {
	ui.Element
	Dismissed bool
	// ActionIndex is the clicked action index, or -1 if none fired this frame.
	ActionIndex int
}

// Notification renders a single BoardUI Notification card matching
// components/base/notification/notification.tsx.
func (k Kit) Notification(c *ui.Context, p NotificationProps) NotificationResult {
	status := p.Status
	if status == "" {
		status = StatusNeutral
	}
	closeLabel := p.CloseLabel
	if closeLabel == "" {
		closeLabel = "Dismiss notification"
	}
	rightPad := float32(44)
	if p.NonDismissible {
		rightPad = 16
	}
	role := ui.RoleStatus
	if status == StatusError {
		role = ui.RoleAlert
	}

	res := NotificationResult{ActionIndex: -1}
	card := ui.Row(c).
		FillWidth().
		AlignItems(ui.Start).
		Gap(12).
		Padding(16, rightPad, 16, 16).
		Radius(16).
		Clip().
		Background(k.Color("background-primary-default")).
		Border(1, k.Color("border-button-default")).
		Role(role)
	k.shadows(card, "dropdown")

	badgeBg, badgeFg := k.notificationStatusColors(status)
	card.Children(func() {
		// 40x40 circular status icon badge (BoardUI styles.visual).
		ui.Box(c.Key("visual")).Size(40, 40).Radius(20).Shrink(0).Center().Background(badgeBg).Children(func() {
			if p.Loading {
				ui.Spinner(c.Key("spin")).Size(20, 20).TextColor(badgeFg).AccessHidden(true)
			} else {
				ui.Icon(c.Key("ico"), notificationIconSVG(status)).Size(20, 20).TextColor(badgeFg).AccessHidden(true)
			}
		})

		// Message content column (BoardUI styles.content).
		ui.Column(c.Key("content")).Grow(1).MinWidth(0).Gap(4).Children(func() {
			ui.Row(c.Key("header")).MinWidth(0).Wrap().AlignItems(ui.Baseline).GapX(8).GapY(2).Children(func() {
				k.Text(c.Key("title"), p.Title, "body-medium").TextColor(k.Color("text-primary"))
				if p.Timestamp != "" {
					k.Text(c.Key("ts"), p.Timestamp, "body-regular").TextColor(k.Color("text-tertiary"))
				}
			})
			if p.Description != "" {
				k.Text(c.Key("desc"), p.Description, "body-regular").TextColor(k.Color("text-secondary"))
			}
			if len(p.Actions) > 0 {
				ui.Row(c.Key("actions")).Margin(6, 0, 0, 0).Wrap().AlignItems(ui.Center).Gap(8).Children(func() {
					for i, act := range p.Actions {
						variant := act.Variant
						if variant == "" {
							if i == 0 {
								variant = "secondary"
							} else {
								variant = "primary"
							}
						}
						btn := k.Button(c.Key(i), act.Label, ButtonProps{
							Size:     "small",
							Variant:  variant,
							Disabled: act.Disabled,
							Busy:     act.Busy,
						})
						if btn.Clicked() {
							res.ActionIndex = i
						}
					}
				})
			}
		})

		// Top-right circular CloseButton (BoardUI styles.close: absolute top-3 end-3).
		if !p.NonDismissible {
			if k.CloseButton(c.Key("close"), CloseButtonProps{Size: "xs", Label: closeLabel}).
				Absolute().Top(12).Right(12).Clicked() {
				res.Dismissed = true
			}
		}

		// 3px bottom countdown bar (BoardUI bg-accent-600).
		if p.ShowBar {
			frac := max(float32(0), min(float32(1), p.Progress))
			if frac > 0 {
				ui.Box(c.Key("countdown")).Absolute().Left(0).Bottom(0).WidthPercent(frac * 100).Height(3).
					Background(k.Color("accent-600")).PassThrough().AccessHidden(true)
			}
		}
	})

	res.Element = card
	return res
}

// Toaster renders the floating BoardUI Toast viewport in a top-level overlay.
// Call it once near the root of the window view after dialogs/sheets.
func (k Kit) Toaster(c *ui.Context, store *ToastStore, p ToasterProps) {
	if store == nil {
		return
	}
	limit := p.Limit
	if limit <= 0 {
		limit = DefaultToastLimit
	}
	defaultDuration := p.Duration
	if defaultDuration == 0 {
		defaultDuration = DefaultToastDuration
	}
	pos := normalizeToastPosition(p.Position)
	ariaLabel := p.Label
	if ariaLabel == "" {
		ariaLabel = "Notifications"
	}
	closeLabel := p.CloseLabel
	if closeLabel == "" {
		closeLabel = "Dismiss notification"
	}

	// Enforce stack limit: oldest toasts leave first.
	if len(store.items) > limit {
		overflow := slices.Clone(store.items[:len(store.items)-limit])
		store.items = store.items[len(store.items)-limit:]
		for _, rec := range overflow {
			if rec.onDismiss != nil {
				rec.onDismiss()
			}
		}
	}

	now := c.Now()
	// Advance active countdowns unless hovered or focused.
	var expired []func()
	kept := store.items[:0]
	for i := range store.items {
		rec := store.items[i]
		if rec.lastTick.IsZero() {
			dur := rec.duration
			if dur == 0 {
				dur = defaultDuration
			}
			if dur > 0 {
				rec.resolvedDuration = dur
				rec.remaining = dur
			} else {
				rec.resolvedDuration = 0
				rec.remaining = 0
			}
			rec.lastTick = now
		} else if rec.resolvedDuration > 0 {
			if !store.paused {
				if elapsed := now.Sub(rec.lastTick); elapsed > 0 {
					rec.remaining -= elapsed
				}
			}
			rec.lastTick = now
			if rec.remaining <= 0 {
				if rec.onDismiss != nil {
					expired = append(expired, rec.onDismiss)
				}
				continue
			}
		}
		kept = append(kept, rec)
	}
	clear(store.items[len(kept):])
	store.items = kept
	for _, fn := range expired {
		fn()
	}

	// Emptying the stack resets pause state so the next toast never starts paused.
	if len(store.items) == 0 {
		store.paused = false
	}

	w, _ := c.Size()
	inset := float32(24)
	if w > 0 && w < 640 {
		inset = 12
	}
	stackWidth := float32(400)
	if w > 0 {
		stackWidth = min(400, max(240, w-24))
	}

	var dismissIDs []string
	var pendingActions []func()
	anyHeld := false
	reduceMotion := c.Preferences().ReduceMotion

	ui.Overlay(c, func() {
		vp := ui.Column(c.Key("boardui.toaster")).
			Absolute().Left(0).Top(0).Right(0).Bottom(0).
			ZIndex(130).
			Padding(inset).
			PassThrough()
		switch pos {
		case PositionTopStart:
			vp.Justify(ui.Start).AlignItems(ui.Start)
		case PositionTopCenter:
			vp.Justify(ui.Start).AlignItems(ui.Center)
		case PositionTopEnd:
			vp.Justify(ui.Start).AlignItems(ui.End)
		case PositionBottomStart:
			vp.Justify(ui.End).AlignItems(ui.Start)
		case PositionBottomCenter:
			vp.Justify(ui.End).AlignItems(ui.Center)
		default:
			vp.Justify(ui.End).AlignItems(ui.End)
		}

		vp.Children(func() {
			stack := ui.Column(c.Key("stack")).
				Width(stackWidth).
				Gap(12).
				Role(ui.RoleRegion).
				Label(ariaLabel).
				PassThrough()
			stack.Children(func() {
				for i := range store.items {
					rec := store.items[i]
					timed := rec.resolvedDuration > 0
					showBar := timed
					var progress float32
					if rec.progress != nil {
						showBar = true
						progress = *rec.progress
					} else if timed {
						progress = float32(rec.remaining) / float32(rec.resolvedDuration)
					}
					itemCloseLabel := closeLabel
					if rec.closeLabel != "" {
						itemCloseLabel = rec.closeLabel
					}
					itemKey := fmt.Sprintf("toast-%s-%d", rec.id, rec.revision)
					res := k.Notification(c.Key(itemKey), NotificationProps{
						Title:          rec.title,
						Description:    rec.description,
						Timestamp:      rec.timestamp,
						Status:         rec.status,
						Loading:        rec.loading,
						Actions:        rec.actions,
						NonDismissible: rec.nonDismissible,
						CloseLabel:     itemCloseLabel,
						Progress:       progress,
						ShowBar:        showBar,
					})
					if !reduceMotion {
						enterY, exitY := float32(12), float32(8)
						if pos == PositionTopStart || pos == PositionTopCenter || pos == PositionTopEnd {
							enterY, exitY = -12, -8
						}
						res.Transition(ui.ElementTransition{
							Duration:     250 * time.Millisecond,
							ExitDuration: 180 * time.Millisecond,
							Position:     true,
							Ease:         ui.EaseOut,
							ExitEase:     ui.EaseOut,
							Enter:        &ui.Motion{Opacity: 0, Y: enterY, Scale: 0.97, Blur: 4},
							Exit:         &ui.Motion{Opacity: 0, Y: exitY, Scale: 0.96, Blur: 3},
						})
					}
					if res.Hovered() || res.FocusWithin() {
						anyHeld = true
					}
					if res.Dismissed {
						dismissIDs = append(dismissIDs, rec.id)
					}
					if res.ActionIndex >= 0 && res.ActionIndex < len(rec.actions) {
						act := rec.actions[res.ActionIndex]
						if act.OnClick != nil {
							pendingActions = append(pendingActions, act.OnClick)
						}
						if !act.KeepOpen {
							dismissIDs = append(dismissIDs, rec.id)
						}
					}
				}
			})
		})
	})

	store.paused = anyHeld && len(store.items) > 0

	for _, fn := range pendingActions {
		fn()
	}
	for _, id := range dismissIDs {
		store.Dismiss(id)
	}
	if len(store.items) == 0 {
		store.paused = false
		return
	}

	// Schedule next frame for active countdown bars and expiration.
	if !store.paused {
		var next time.Duration
		for i := range store.items {
			rec := store.items[i]
			if rec.resolvedDuration <= 0 {
				continue
			}
			step := 33 * time.Millisecond
			if reduceMotion {
				step = 100 * time.Millisecond
			}
			if rec.remaining < step {
				step = max(time.Millisecond, rec.remaining)
			}
			if next == 0 || step < next {
				next = step
			}
		}
		if next > 0 {
			c.After(next)
		}
	}
}

func normalizeToastPosition(pos NotificationPosition) NotificationPosition {
	switch pos {
	case PositionTopStart, "top-left":
		return PositionTopStart
	case PositionTopCenter:
		return PositionTopCenter
	case PositionTopEnd, "top-right":
		return PositionTopEnd
	case PositionBottomStart, "bottom-left":
		return PositionBottomStart
	case PositionBottomCenter:
		return PositionBottomCenter
	case PositionBottomEnd, "bottom-right", "":
		return PositionBottomEnd
	default:
		panic("boardui: unknown toast position " + string(pos))
	}
}

func (k Kit) notificationStatusColors(status NotificationStatus) (bg, fg ui.Color) {
	switch status {
	case StatusNeutral, "":
		return k.Color("background-tertiary-default"), k.Color("text-secondary")
	case StatusInformation:
		return k.Color("notification-information-background"), k.Color("notification-information-foreground")
	case StatusSuccess:
		return k.Color("notification-success-background"), k.Color("notification-success-foreground")
	case StatusWarning:
		return k.Color("status-yellow-background"), k.Color("status-yellow-text")
	case StatusError:
		return k.Color("notification-error-background"), k.Color("notification-error-foreground")
	default:
		panic("boardui: unknown notification status " + string(status))
	}
}

// Official Remix Icon 4.9.0 filled glyphs used by BoardUI Notification/Toast
// (RiNotification3Fill, RiInformationFill, RiCheckboxCircleFill, RiAlertFill, RiErrorWarningFill).
var notificationIconSources = map[NotificationStatus]string{
	StatusNeutral:     `<svg viewBox="0 0 24 24" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><path d="M20 17H22V19H2V17H4V10C4 5.58172 7.58172 2 12 2C16.4183 2 20 5.58172 20 10V17ZM9 21H15V23H9V21Z"/></svg>`,
	StatusInformation: `<svg viewBox="0 0 24 24" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><path d="M12 22C6.47715 22 2 17.5228 2 12C2 6.47715 6.47715 2 12 2C17.5228 2 22 6.47715 22 12C22 17.5228 17.5228 22 12 22ZM11 11V17H13V11H11ZM11 7V9H13V7H11Z"/></svg>`,
	StatusSuccess:     `<svg viewBox="0 0 24 24" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><path d="M12 22C17.5228 22 22 17.5228 22 12C22 6.47715 17.5228 2 12 2C6.47715 2 2 6.47715 2 12C2 17.5228 6.47715 22 12 22ZM17.4571 9.45711L11 15.9142L6.79289 11.7071L8.20711 10.2929L11 13.0858L16.0429 8.04289L17.4571 9.45711Z"/></svg>`,
	StatusWarning:     `<svg viewBox="0 0 24 24" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><path d="M12.8659 3.00017L22.3922 19.5002C22.6684 19.9785 22.5045 20.5901 22.0262 20.8662C21.8742 20.954 21.7017 21.0002 21.5262 21.0002H2.47363C1.92135 21.0002 1.47363 20.5525 1.47363 20.0002C1.47363 19.8246 1.51984 19.6522 1.60761 19.5002L11.1339 3.00017C11.41 2.52187 12.0216 2.358 12.4999 2.63414C12.6519 2.72191 12.7782 2.84815 12.8659 3.00017ZM10.9999 16.0002V18.0002H12.9999V16.0002H10.9999ZM10.9999 9.00017V14.0002H12.9999V9.00017H10.9999Z"/></svg>`,
	StatusError:       `<svg viewBox="0 0 24 24" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><path d="M12 22C6.47715 22 2 17.5228 2 12C2 6.47715 6.47715 2 12 2C17.5228 2 22 6.47715 22 12C22 17.5228 17.5228 22 12 22ZM11 15V17H13V15H11ZM11 7V13H13V7H11Z"/></svg>`,
}

var (
	notificationIconOnce  sync.Once
	notificationIconCache map[NotificationStatus]*ui.SVG
)

func notificationIconSVG(status NotificationStatus) *ui.SVG {
	notificationIconOnce.Do(func() {
		notificationIconCache = make(map[NotificationStatus]*ui.SVG, len(notificationIconSources))
		for st, src := range notificationIconSources {
			svg, err := ui.ParseSVG([]byte(src))
			if err != nil {
				panic(fmt.Sprintf("boardui: notification icon %s: %v", st, err))
			}
			notificationIconCache[st] = svg
		}
	})
	if svg, ok := notificationIconCache[status]; ok {
		return svg
	}
	return notificationIconCache[StatusNeutral]
}
