// Package notifications owns Desktop system notification delivery, routing,
// policy checks, deduplication, and pending-request monitoring.
package notifications

import (
	"context"
	"crypto/rand"
	"errors"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo"

	"github.com/uniclipboard/unic/app/desktop/internal/i18n"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/internal/pairnotice"
)

// Category identifies a logical notification class governed by user preferences.
type Category string

const (
	CategoryDevicePairing Category = "device_pairing"
)

// Message describes one desktop system notification and its click target route.
type Message struct {
	ID       string
	Category Category
	Title    string
	Subtitle string
	Body     string
	Route    string
	Group    string
	Silent   bool
}

// PendingSummary captures the current pending device approval count for tray
// menus, tooltips, and OS badge indicators.
type PendingSummary struct {
	Count     int
	FirstID   string
	FirstName string
}

// Deliverer abstracts native OS notification dispatch so notification policy,
// deduplication, and routing are deterministic and testable.
type Deliverer interface {
	Supported() bool
	Deliver(ctx context.Context, msg Message) error
}

// MyGoDeliverer delivers notifications via mygo's native notification backend.
type MyGoDeliverer struct{}

func (MyGoDeliverer) Supported() bool { return mygo.NotificationsSupported() }

func (MyGoDeliverer) Deliver(ctx context.Context, msg Message) error {
	if !mygo.NotificationsSupported() {
		return errors.ErrUnsupported
	}
	n := mygo.NewNotification(mygo.NotificationOptions{
		ID:       msg.ID,
		Title:    msg.Title,
		Subtitle: msg.Subtitle,
		Body:     msg.Body,
		Group:    msg.Group,
		Silent:   msg.Silent,
	})
	return n.ShowContext(ctx)
}

const (
	defaultCooldown = 5 * time.Second
	maxTrackedIDs   = 128
)

// Service coordinates desktop notification policy, deduplication, delivery,
// and click-to-navigate routing.
type Service struct {
	deliverer  Deliverer
	configFn   func() config.Desktop
	onNavigate func(route string)
	onError    func(error)
	cooldown   time.Duration
	now        func() time.Time

	mu       sync.Mutex
	routes   map[string]string
	lastSent map[string]time.Time
}

// Options configures a notification Service.
type Options struct {
	Deliverer  Deliverer
	Config     func() config.Desktop
	OnNavigate func(route string)
	OnError    func(error)
	Cooldown   time.Duration
}

// New creates a Desktop notification Service.
func New(opts Options) *Service {
	deliverer := opts.Deliverer
	if deliverer == nil {
		deliverer = MyGoDeliverer{}
	}
	cfgFn := opts.Config
	if cfgFn == nil {
		cfgFn = func() config.Desktop { return config.Default().Desktop }
	}
	cooldown := opts.Cooldown
	if cooldown <= 0 {
		cooldown = defaultCooldown
	}
	return &Service{
		deliverer:  deliverer,
		configFn:   cfgFn,
		onNavigate: opts.OnNavigate,
		onError:    opts.OnError,
		cooldown:   cooldown,
		now:        time.Now,
		routes:     make(map[string]string),
		lastSent:   make(map[string]time.Time),
	}
}

// Supported reports whether the active deliverer can show OS notifications.
func (s *Service) Supported() bool {
	return s.deliverer != nil && s.deliverer.Supported()
}

// Allowed reports whether user configuration enables notifications for cat.
func (s *Service) Allowed(cat Category) bool {
	cfg := s.configFn().Notifications
	switch cat {
	case CategoryDevicePairing:
		return cfg.DevicePairingEnabled()
	default:
		return cfg.EnabledOrDefault()
	}
}

// Notify evaluates policy and cooldown deduplication, records the click route,
// and dispatches msg via the configured deliverer.
func (s *Service) Notify(ctx context.Context, msg Message) (bool, error) {
	if !s.Allowed(msg.Category) {
		return false, nil
	}
	if !s.Supported() {
		return false, nil
	}
	if msg.ID != "" {
		now := s.now()
		s.mu.Lock()
		if prev, ok := s.lastSent[msg.ID]; ok && now.Sub(prev) < s.cooldown {
			s.mu.Unlock()
			return false, nil
		}
		if len(s.lastSent) >= maxTrackedIDs {
			clear(s.lastSent)
		}
		if len(s.routes) >= maxTrackedIDs {
			clear(s.routes)
		}
		s.lastSent[msg.ID] = now
		if msg.Route != "" {
			s.routes[msg.ID] = msg.Route
		}
		s.mu.Unlock()
	}
	if err := s.deliverer.Deliver(ctx, msg); err != nil {
		if s.onError != nil {
			s.onError(err)
		}
		return false, err
	}
	return true, nil
}

// SendTest dispatches an explicit user-initiated test notification regardless
// of category switches so the user can trigger OS permission prompts or verify
// system notification delivery from Settings.
func (s *Service) SendTest(ctx context.Context) error {
	if !s.Supported() {
		return errors.ErrUnsupported
	}
	lang := s.configFn().Language
	id := "test:" + rand.Text()
	msg := Message{
		ID:    id,
		Title: i18n.Translate(lang, "通知测试"),
		Body:  i18n.Translate(lang, "系统通知已正常工作。点击可返回通知设置。"),
		Route: "settings:notifications",
		Group: "system-test",
	}
	s.mu.Lock()
	if len(s.routes) >= maxTrackedIDs {
		clear(s.routes)
	}
	s.routes[id] = msg.Route
	s.mu.Unlock()
	return s.deliverer.Deliver(ctx, msg)
}

// HandleClick resolves a clicked notification ID to its target route and
// invokes the navigation callback. Deterministic prefix fallbacks keep clicks
// working even after a process restart.
func (s *Service) HandleClick(id string) {
	if s.onNavigate == nil {
		return
	}
	route := s.ResolveRoute(id)
	if route != "" {
		s.onNavigate(route)
	}
}

// ResolveRoute maps a notification ID to a GUI navigation route.
func (s *Service) ResolveRoute(id string) string {
	s.mu.Lock()
	route := s.routes[id]
	s.mu.Unlock()
	if route != "" {
		return route
	}
	switch {
	case strings.HasPrefix(id, "pending:"):
		deviceID := strings.TrimPrefix(id, "pending:")
		if strings.HasPrefix(deviceID, "id:") {
			return "devices:" + strings.TrimPrefix(deviceID, "id:")
		}
		return "devices"
	case strings.HasPrefix(id, "test:"):
		return "settings:notifications"
	default:
		return ""
	}
}

// BuildPendingMessage constructs the localized notification for a pending
// device approval event.
func BuildPendingMessage(language, deviceID, peerName, info string) Message {
	title, body := pairnotice.Text(language, peerName, info)
	id := "pending:peer:" + peerName
	route := "devices"
	if deviceID != "" {
		id = "pending:id:" + deviceID
		route = "devices:" + deviceID
	}
	return Message{
		ID:       id,
		Category: CategoryDevicePairing,
		Title:    title,
		Body:     body,
		Route:    route,
		Group:    "device-pairing",
	}
}

// Watch subscribes to local service events, dispatches system notifications for
// eligible events, and reports pending device approval summaries to onPending.
func (s *Service) Watch(ctx context.Context, client *api.Client, onPending func(PendingSummary)) {
	if client == nil {
		return
	}
	var lastSummary PendingSummary
	haveSummary := false
	publishPending := func(summary PendingSummary) {
		if onPending == nil {
			return
		}
		if !haveSummary || summary != lastSummary {
			haveSummary = true
			lastSummary = summary
			onPending(summary)
		}
	}
	syncPending := func() []api.PendingPeer {
		pctx, cancel := context.WithTimeout(ctx, 5*time.Second)
		defer cancel()
		st, err := client.Status(pctx)
		if err != nil {
			return nil
		}
		summary := PendingSummary{Count: len(st.Pending)}
		if len(st.Pending) > 0 {
			summary.FirstID = st.Pending[0].ID
			summary.FirstName = st.Pending[0].Name
		}
		publishPending(summary)
		return st.Pending
	}

	for ctx.Err() == nil {
		syncPending()
		_ = client.Events(ctx, func(ev api.Event) {
			switch ev.Kind {
			case "pending":
				pending := syncPending()
				deviceID := ""
				for _, p := range pending {
					if p.Name == ev.Peer {
						deviceID = p.ID
						break
					}
				}
				if deviceID == "" && len(pending) == 1 {
					deviceID = pending[0].ID
				}
				msg := BuildPendingMessage(s.configFn().Language, deviceID, ev.Peer, ev.Info)
				_, _ = s.Notify(ctx, msg)
			case "peers", "peer-up", "peer-down":
				syncPending()
			}
		})
		select {
		case <-ctx.Done():
			return
		case <-time.After(2 * time.Second):
		}
	}
}
