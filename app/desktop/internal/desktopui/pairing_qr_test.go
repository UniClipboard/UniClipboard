package desktopui

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"image/png"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/rendezvous"
)

func TestEncodePairingQRImage(t *testing.T) {
	realTicket := rendezvous.Ticket{
		Transport: rendezvous.TransportPeerSessions,
		Backend:   "builtin.tailcat",
		Device:    bytes.Repeat([]byte{0x42}, 32),
		Access:    []byte("tailcat:nodekey:abcdef0123456789abcdef0123456789@192.168.1.20:41641"),
		Token:     bytes.Repeat([]byte{0x9a}, 16),
	}.Text()

	for _, raw := range []string{
		"482-913",
		" 123456 ",
		realTicket,
		"unic-pair:AQ7xK2m9pR4vN8qL…",
	} {
		img := encodePairingQRImage(raw)
		if img == nil {
			t.Fatalf("encodePairingQRImage(%q) = nil, want QR image", raw)
		}
		b := img.Bounds()
		if b.Dx() != b.Dy() || b.Dx() < pairingQRMinPixels {
			t.Fatalf("encodePairingQRImage(%q) bounds = %v, want square >= %d", raw, b, pairingQRMinPixels)
		}
		for _, pt := range [][2]int{{0, 0}, {b.Dx() - 1, 0}, {0, b.Dy() - 1}, {b.Dx() - 1, b.Dy() - 1}} {
			c := img.NRGBAAt(pt[0], pt[1])
			if c.R != 255 || c.G != 255 || c.B != 255 || c.A != 255 {
				t.Fatalf("quiet zone corner %v = %+v, want pure white", pt, c)
			}
		}
		hasBlack := false
		for i := 0; i < len(img.Pix); i += 4 {
			if img.Pix[i] == 0 && img.Pix[i+1] == 0 && img.Pix[i+2] == 0 && img.Pix[i+3] == 255 {
				hasBlack = true
				break
			}
		}
		if !hasBlack {
			t.Fatalf("encodePairingQRImage(%q) has no dark modules", raw)
		}
	}

	oversized := "unic-pair:" + strings.Repeat("A", maxPairingQRBytes)
	for _, bad := range []string{
		"",
		"   ",
		"···-···",
		"12345",
		"https://example.com",
		"unic-pair:with space",
		oversized,
	} {
		if img := encodePairingQRImage(bad); img != nil {
			t.Fatalf("encodePairingQRImage(%q) unexpectedly produced an image", bad)
		}
	}
}

func TestPairingSheetQRCodeAndTicketModes(t *testing.T) {
	evidenceDir := os.Getenv("UNIC_PAIRING_QR_EVIDENCE_DIR")
	if evidenceDir != "" {
		if err := os.MkdirAll(evidenceDir, 0o755); err != nil {
			t.Fatal(err)
		}
	}

	for _, theme := range []string{"light", "dark"} {
		t.Run(theme, func(t *testing.T) {
			d := newDemo("devices", theme)
			tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: theme == "dark"})
			if evidenceDir != "" {
				tt.SetScale(2)
			}
			save := func(name string) {
				if evidenceDir == "" {
					return
				}
				f, err := os.Create(filepath.Join(evidenceDir, name))
				if err != nil {
					t.Fatal(err)
				}
				defer func() { _ = f.Close() }()
				if err := png.Encode(f, tt.Image()); err != nil {
					t.Fatal(err)
				}
			}

			if err := tt.Click("添加设备"); err != nil {
				t.Fatal(err)
			}
			tt.Advance(350 * time.Millisecond)

			// 1. Default 6-digit code mode displays a scannable QR code for the active pairing code.
			qrNode, ok := tt.AccessByRole(ui.RoleImage, "配对二维码")
			if !ok {
				t.Fatal("code mode missing pairing QR image")
			}
			if qrNode.Bounds.W < pairingCodeQRSize-1 || qrNode.Bounds.H < pairingCodeQRSize-1 {
				t.Fatalf("code mode QR bounds = %+v, want >= %d", qrNode.Bounds, pairingCodeQRSize)
			}
			firstCode := d.inviteCode
			if d.pairingQR.payload != firstCode || d.pairingQR.bitmap == nil {
				t.Fatalf("code mode QR cache = %+v, want payload %q", d.pairingQR, firstCode)
			}
			dlgCode, ok := tt.AccessByRole(ui.RoleDialog, "")
			if !ok {
				t.Fatal("missing pairing dialog in code mode")
			}
			if dlgCode.Bounds.H >= 688 {
				t.Fatalf("code mode dialog overflowed 720p viewport: H=%v", dlgCode.Bounds.H)
			}
			save(fmt.Sprintf("pairing-code-%s.png", theme))

			// 2. Refreshing the pairing code updates the cached QR payload in lockstep.
			if err := tt.Click("刷新"); err != nil {
				t.Fatal(err)
			}
			tt.Frame()
			if d.pairingQR.payload != d.inviteCode || d.pairingQR.bitmap == nil {
				t.Fatalf("after refresh QR cache = %+v, want payload %q", d.pairingQR, d.inviteCode)
			}

			// 3. Switching to full ticket mode displays a larger QR code for the ticket payload.
			if err := tt.Click("跨网或命令行配对？改用完整票据 →"); err != nil {
				t.Fatal(err)
			}
			tt.Advance(350 * time.Millisecond)

			ticketQR, ok := tt.AccessByRole(ui.RoleImage, "配对二维码")
			if !ok {
				t.Fatal("ticket mode missing pairing QR image")
			}
			if ticketQR.Bounds.W < pairingTicketQRSize-1 || ticketQR.Bounds.H < pairingTicketQRSize-1 {
				t.Fatalf("ticket mode QR bounds = %+v, want >= %d", ticketQR.Bounds, pairingTicketQRSize)
			}
			if !strings.HasPrefix(d.pairingQR.payload, "unic-pair:") || d.pairingQR.bitmap == nil {
				t.Fatalf("ticket mode QR cache = %+v, want unic-pair: payload", d.pairingQR)
			}
			dlgTicket, ok := tt.AccessByRole(ui.RoleDialog, "")
			if !ok {
				t.Fatal("missing pairing dialog in ticket mode")
			}
			if dlgTicket.Bounds.H >= 688 {
				t.Fatalf("ticket mode dialog overflowed 720p viewport: H=%v", dlgTicket.Bounds.H)
			}
			save(fmt.Sprintf("pairing-ticket-%s.png", theme))

			// 4. Returning to code mode replaces the cached ticket secret with the 6-digit code.
			if err := tt.Click("← 返回 6 位配对码"); err != nil {
				t.Fatal(err)
			}
			tt.Advance(350 * time.Millisecond)
			if d.pairingQR.payload != d.inviteCode {
				t.Fatalf("returning to code mode retained stale payload %q, want %q", d.pairingQR.payload, d.inviteCode)
			}

			// 5. Switching to the connect tab clears the cached QR bitmap and payload.
			if err := tt.Click("输入配对码"); err != nil {
				t.Fatal(err)
			}
			tt.Advance(350 * time.Millisecond)
			if d.pairingQR.bitmap != nil || d.pairingQR.payload != "" {
				t.Fatalf("switching to connect tab retained QR cache: %+v", d.pairingQR)
			}

			// 6. Re-entering invite and closing via Escape clears the cached QR bitmap and payload.
			if err := tt.Click("出示配对码"); err != nil {
				t.Fatal(err)
			}
			tt.Advance(350 * time.Millisecond)
			if d.pairingQR.bitmap == nil || d.pairingQR.payload == "" {
				t.Fatal("re-entering invite did not restore QR bitmap")
			}
			tt.Key(0, ui.KeyEscape)
			tt.Advance(350 * time.Millisecond)
			if d.pairingQR.bitmap != nil || d.pairingQR.payload != "" {
				t.Fatalf("closing pairing sheet retained QR cache: %+v", d.pairingQR)
			}
		})
	}
}

func TestPairingSheetQRLiveLifecycleAndPlaceholder(t *testing.T) {
	realTicket1 := rendezvous.Ticket{
		Transport: rendezvous.TransportPeerSessions,
		Backend:   "builtin.tailcat",
		Device:    bytes.Repeat([]byte{0x11}, 32),
		Access:    []byte("tailcat:nodekey:1111111111111111@10.0.0.2:41641"),
		Token:     bytes.Repeat([]byte{0x22}, 16),
	}.Text()
	realTicket2 := rendezvous.Ticket{
		Transport: rendezvous.TransportPeerSessions,
		Backend:   "builtin.tailcat",
		Device:    bytes.Repeat([]byte{0x11}, 32),
		Access:    []byte("tailcat:nodekey:1111111111111111@10.0.0.2:41641"),
		Token:     bytes.Repeat([]byte{0x33}, 16),
	}.Text()

	var mu sync.Mutex
	codeSeq := 0
	ticketSeq := 0

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	client := api.NewClient("/nonexistent-pairing-qr.sock")
	client.WrapTransport(func(_ http.RoundTripper) http.RoundTripper {
		return historySendTransport(func(r *http.Request) (*http.Response, error) {
			mu.Lock()
			defer mu.Unlock()
			var body any
			switch r.Method + " " + r.URL.Path {
			case "POST /v1/code":
				codeSeq++
				code := fmt.Sprintf("%03d-%03d", 600+codeSeq, 300+codeSeq)
				body = api.Code{Code: code, ExpiresAtMs: time.Now().Add(5 * time.Minute).UnixMilli()}
			case "POST /v1/pairing-ticket":
				ticketSeq++
				tix := realTicket1
				if ticketSeq > 1 {
					tix = realTicket2
				}
				body = map[string]string{"ticket": tix}
			case "DELETE /v1/code":
				body = map[string]any{}
			default:
				body = map[string]any{}
			}
			raw, _ := json.Marshal(body)
			return &http.Response{
				StatusCode: http.StatusOK,
				Body:       io.NopCloser(bytes.NewReader(raw)),
				Header:     make(http.Header),
			}, nil
		})
	})

	d := newDemo("devices", "light")
	d.inviteCode = ""
	d.pending = nil
	d.live = &liveState{
		ctx:            ctx,
		cancel:         cancel,
		client:         client,
		jobs:           make(chan func(context.Context) func(), 8),
		results:        make(chan func(), 16),
		changed:        make(chan struct{}, 1),
		recovery:       make(chan struct{}, 1),
		pairingBackend: "builtin.tailcat",
		pending:        map[string]api.PendingPeer{},
		peers:          map[string]api.PeerStatus{},
		// Prevent unrelated background history/catalogue fetches in this focused unit test.
		nextRefresh:      time.Now().Add(time.Hour),
		pageRetry:        time.Now().Add(time.Hour),
		searchSlots:      make(chan struct{}, 4),
		key:              d.liveQueryKey(),
		codeAttempted:    true,
		ticketAttempted:  true,
		taskKnownPending: map[string]int64{},
	}
	go d.live.work()

	drain := func(label string) {
		t.Helper()
		deadline := time.Now().Add(5 * time.Second)
		for time.Now().Before(deadline) {
			for {
				select {
				case f := <-d.live.results:
					f()
				default:
					goto checked
				}
			}
		checked:
			if !d.live.busy {
				return
			}
			time.Sleep(10 * time.Millisecond)
		}
		t.Fatalf("timeout waiting for %s", label)
	}

	d.deviceTask = "invite"
	d.inviteMode = "code"
	tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{})
	tt.Advance(350 * time.Millisecond)

	// 1. Before any code exists, placeholder is shown and no QR image node is present.
	if _, ok := tt.AccessByRole(ui.RoleImage, "配对二维码"); ok {
		t.Fatal("empty code unexpectedly rendered a QR image")
	}
	if !tt.HasText("暂无二维码") {
		t.Fatal("empty code missing QR placeholder label")
	}
	dlgPlaceholder, _ := tt.AccessByRole(ui.RoleDialog, "")

	// 2. Request a live pairing code; QR appears without changing the dialog height.
	d.newLiveCode()
	drain("newLiveCode 1")
	tt.Frame()
	if d.inviteCode != "601-301" {
		t.Fatalf("inviteCode = %q, want 601-301", d.inviteCode)
	}
	if _, ok := tt.AccessByRole(ui.RoleImage, "配对二维码"); !ok {
		t.Fatal("live code did not render QR image")
	}
	if d.pairingQR.payload != "601-301" {
		t.Fatalf("pairingQR payload = %q, want 601-301", d.pairingQR.payload)
	}
	dlgReady, _ := tt.AccessByRole(ui.RoleDialog, "")
	if dlgReady.Bounds.H != dlgPlaceholder.Bounds.H {
		t.Fatalf("dialog height jumped between placeholder (%v) and QR ready (%v)", dlgPlaceholder.Bounds.H, dlgReady.Bounds.H)
	}

	// 3. Switch to ticket mode and fetch the live ticket; QR switches to realTicket1.
	d.inviteMode = "ticket"
	d.live.ticketAttempted = false
	d.newLiveTicket()
	drain("newLiveTicket 1")
	tt.Advance(350 * time.Millisecond)
	if d.live.ticket != realTicket1 {
		t.Fatalf("live ticket = %q, want realTicket1", d.live.ticket)
	}
	if d.pairingQR.payload != realTicket1 {
		t.Fatalf("ticket QR payload = %q, want realTicket1", d.pairingQR.payload)
	}

	// 4. Refreshing code invalidates cached live.ticket so a later ticket switch fetches a fresh ticket.
	d.inviteMode = "code"
	d.newLiveCode()
	drain("newLiveCode 2")
	tt.Frame()
	if d.inviteCode != "602-302" {
		t.Fatalf("refreshed inviteCode = %q, want 602-302", d.inviteCode)
	}
	if d.live.ticket != "" || d.live.ticketAttempted {
		t.Fatalf("newLiveCode did not clear stale ticket state: ticket=%q attempted=%v", d.live.ticket, d.live.ticketAttempted)
	}
	if d.pairingQR.payload != "602-302" {
		t.Fatalf("refreshed QR payload = %q, want 602-302", d.pairingQR.payload)
	}

	// 5. Entering inline pending approval immediately clears the QR bitmap and secret payload.
	d.pending = []pendingDevice{{
		id: "peer-new", name: "iPhone", revision: 1, byCode: true,
		fingerprint: "4A8C 2E91 7B30 D6F5 08B1 E49A 53C2 7F6D",
		paths:       []pendingPath{{id: "bp1", instanceID: "builtin.tailcat", state: "pending", revision: 1}},
	}}
	tt.Frame()
	if d.pairingQR.bitmap != nil || d.pairingQR.payload != "" {
		t.Fatalf("inline approval did not clear QR cache: %+v", d.pairingQR)
	}
}
