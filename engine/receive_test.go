package engine

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func updateSettings(t *testing.T, e *Engine, f func(*Settings)) {
	t.Helper()
	s := state(t, e).Settings
	f(&s.Value)
	if _, err := e.UpdateSettings(bg, UpdateSettingsRequest{ExpectedRevision: s.Revision, Value: s.Value}); err != nil {
		t.Fatal(err)
	}
}

// pairedPair starts two engines (b built with mutate) and pairs them.
func pairedPair(t *testing.T, mutateB func(*Options)) (a, b node) {
	t.Helper()
	a, b = newContentNode(t, nil), newContentNode(t, mutateB)
	start(t, a)
	start(t, b)
	pairTwo(t, a, b)
	return a, b
}

func sendText(t *testing.T, from, to node, s string) Operation {
	t.Helper()
	h := importText(t, from.e, s)
	defer func() { _ = from.e.ReleaseContent(bg, h.LeaseID) }()
	id, err := submit(from.e, h)
	if err != nil {
		t.Fatal(err)
	}
	return waitSend(t, from.e, id)
}

func received(e *Engine) []ReceivedContent { r, _ := e.ListReceived(bg); return r }

func waitReceived(t *testing.T, e *Engine, n int) []ReceivedContent {
	t.Helper()
	eventually(t, "received list", func() bool { return len(received(e)) == n })
	return received(e)
}

func TestReceiveBetweenEngines(t *testing.T) {
	a, b := pairedPair(t, nil)
	sub, _ := b.e.SubscribeEvents(bg)
	defer func() { _ = sub.Close() }()

	img := pngBytes(t)
	h, err := a.e.ImportContent(bg, ContentInput{
		Representations: []RepresentationInput{rep(MimeText, "héllo"), rep(MimeHTML, "<i>x</i>"),
			{MIME: MimePNG, Size: int64(len(img)), Reader: strings.NewReader(string(img))}},
		Files: []FileInput{fileIn("../report.txt", "file body")},
	})
	if err != nil {
		t.Fatal(err)
	}
	id, _ := submit(a.e, h)
	if o := waitSend(t, a.e, id); o.Outcome != OutcomeSuccess {
		t.Fatalf("send = %+v", o)
	}
	rs := waitReceived(t, b.e, 1)
	r := rs[0]
	if r.PeerID != selfID(a.e) || r.ApplyState != ApplyAvailable || r.ExpiresAtMs <= r.ReceivedAtMs || len(r.Content.Parts) != 4 {
		t.Fatalf("received = %+v", r)
	}
	if st := state(t, b.e); len(st.Received) != 1 || st.Received[0].ReceiptID != r.ReceiptID {
		t.Fatalf("State.Received = %+v", st.Received)
	}

	lease, err := b.e.RetainContent(bg, r.Content.ID)
	if err != nil {
		t.Fatal(err)
	}
	var file PartDescriptor
	for _, p := range lease.Content.Parts {
		switch p.MIME {
		case MimeText:
			if readAll(t, b.e, lease.LeaseID, p.ID) != "héllo" {
				t.Fatal("text differs")
			}
		case MimePNG:
			if readAll(t, b.e, lease.LeaseID, p.ID) != string(img) {
				t.Fatal("png differs")
			}
		}
		if p.Kind == PartFile {
			file = p
		}
	}
	if file.Name != "report.txt" || readAll(t, b.e, lease.LeaseID, file.ID) != "file body" {
		t.Fatalf("file = %+v", file)
	}
	path, err := b.e.LocalFilePath(bg, lease.LeaseID, file.ID)
	if err != nil || !strings.HasPrefix(path, b.e.recvDir()+string(filepath.Separator)) {
		t.Fatalf("LocalFilePath = %q, %v", path, err)
	}
	if got, _ := os.ReadFile(path); string(got) != "file body" {
		t.Fatal("file on disk differs")
	}

	// A received event came through, without any content in it.
	var saw bool
	for !saw {
		ev, err := sub.Next(bg)
		if err != nil {
			t.Fatal(err)
		}
		saw = ev.Kind == EventReceived && len(ev.Objects) == 1 && ev.Objects[0].ID == string(r.ReceiptID)
	}
}

func TestDismissAndRetain(t *testing.T) {
	a, b := pairedPair(t, nil)
	sendText(t, a, b, "keep me")
	r := waitReceived(t, b.e, 1)[0]
	lease, _ := b.e.RetainContent(bg, r.Content.ID)

	for i := 0; i < 2; i++ { // idempotent
		if err := b.e.DismissReceived(bg, r.ReceiptID); err != nil {
			t.Fatal(err)
		}
	}
	if len(received(b.e)) != 0 {
		t.Fatal("still listed")
	}
	if readAll(t, b.e, lease.LeaseID, lease.Content.Parts[0].ID) != "keep me" {
		t.Fatal("a retained lease lost its content on dismiss")
	}
	_ = b.e.ReleaseContent(bg, lease.LeaseID)
	if stored(b.e) != 0 || len(b.e.cs.contents) != 0 {
		t.Fatalf("content not freed: stored=%d", stored(b.e))
	}
}

func TestReceivedListIsBounded(t *testing.T) {
	a, b := pairedPair(t, func(o *Options) { o.Resources.MaxReceived = 2 })
	for _, s := range []string{"one", "two", "three"} {
		sendText(t, a, b, s)
		eventually(t, s, func() bool { return len(received(b.e)) > 0 && textOf(t, b.e, received(b.e)[0]) == s })
	}
	rs := received(b.e)
	if len(rs) != 2 || textOf(t, b.e, rs[0]) != "three" || textOf(t, b.e, rs[1]) != "two" {
		t.Fatalf("list = %d entries", len(rs))
	}
	if len(b.e.cs.contents) != 2 {
		t.Fatalf("the evicted content was not freed: %d contents", len(b.e.cs.contents))
	}
}

func textOf(t *testing.T, e *Engine, r ReceivedContent) string {
	t.Helper()
	l, err := e.RetainContent(bg, r.Content.ID)
	if err != nil {
		return ""
	}
	defer func() { _ = e.ReleaseContent(bg, l.LeaseID) }()
	for _, p := range l.Content.Parts {
		if p.MIME == MimeText {
			return readAll(t, e, l.LeaseID, p.ID)
		}
	}
	return ""
}

func TestReceivedEntriesExpire(t *testing.T) {
	a, b := pairedPair(t, func(o *Options) { o.Resources.ReceivedTTL = 400 * time.Millisecond })
	sub, _ := b.e.SubscribeEvents(bg)
	defer func() { _ = sub.Close() }()
	sendText(t, a, b, "fleeting")
	waitReceived(t, b.e, 1)
	eventually(t, "expiry", func() bool { return len(received(b.e)) == 0 })
	eventually(t, "content freed", func() bool {
		b.e.cs.mu.Lock()
		defer b.e.cs.mu.Unlock()
		return len(b.e.cs.contents) == 0 && b.e.cs.stored == 0
	})
	// The change was announced, so a UI does not have to poll for it.
	var events int
	for {
		ev, err := sub.Next(bg)
		if err != nil {
			t.Fatal(err)
		}
		if ev.Kind == EventReceived {
			if events++; events == 2 { // arrival, then expiry
				break
			}
		}
	}
}

func TestStorageIsMadeByDroppingOldEntries(t *testing.T) {
	a, b := pairedPair(t, func(o *Options) {
		o.Settings = DefaultSettings()
		o.Settings.Limits.MaxTotalBytes = 200
		o.Resources.MaxStoredBytes = 200
	})
	sub, _ := b.e.SubscribeEvents(bg)
	defer func() { _ = sub.Close() }()
	big := func(c string) string { return strings.Repeat(c, 150) }

	sendText(t, a, b, big("a"))
	waitReceived(t, b.e, 1)
	// The second does not fit next to the first, but the first is only listed:
	// it gives way.
	sendText(t, a, b, big("b"))
	eventually(t, "second replaces first", func() bool {
		rs := received(b.e)
		return len(rs) == 1 && textOf(t, b.e, rs[0]) == big("b")
	})

	// Now someone holds the newest: nothing can be dropped, so the next one is
	// refused, the connection survives and a warning is raised.
	rs := received(b.e)
	held, _ := b.e.RetainContent(bg, rs[0].Content.ID)
	sendText(t, a, b, big("c"))
	var warned bool
	for !warned {
		ev, err := sub.Next(bg)
		if err != nil {
			t.Fatal(err)
		}
		warned = ev.Kind == EventResourceWarning
	}
	if rs := received(b.e); len(rs) != 1 || textOf(t, b.e, rs[0]) != big("b") {
		t.Fatal("the held content must stay listed")
	}
	if stored(b.e) != 150 {
		t.Fatalf("stored = %d: the refused clip's reservation leaked", stored(b.e))
	}
	// Releasing makes room again, and the link still works.
	_ = b.e.ReleaseContent(bg, held.LeaseID)
	sendText(t, a, b, "small")
	eventually(t, "link still works", func() bool {
		rs := received(b.e)
		return len(rs) > 0 && textOf(t, b.e, rs[0]) == "small"
	})
}

func TestReceiverHonoursItsOwnSettings(t *testing.T) {
	a, b := pairedPair(t, nil)

	// Sync paused: read and dropped, connection kept.
	updateSettings(t, b.e, func(s *Settings) { s.Sync.Enabled = false })
	sendText(t, a, b, "dropped")
	time.Sleep(200 * time.Millisecond)
	if len(received(b.e)) != 0 || stored(b.e) != 0 {
		t.Fatalf("paused receiver kept content (stored %d)", stored(b.e))
	}
	updateSettings(t, b.e, func(s *Settings) {
		s.Sync.Enabled = true
		policy(BackendTailcat, func(p *BackendSyncPolicy) { p.Text = false })(s)
	})

	// Text off: only the file of a mixed clip is kept; a text-only clip vanishes.
	h, _ := a.e.ImportContent(bg, ContentInput{Representations: []RepresentationInput{rep(MimeText, "words")}, Files: []FileInput{fileIn("f.bin", "file")}})
	id, _ := submit(a.e, h)
	waitSend(t, a.e, id)
	r := waitReceived(t, b.e, 1)[0]
	if len(r.Content.Parts) != 1 || r.Content.Parts[0].Kind != PartFile || stored(b.e) != 4 {
		t.Fatalf("received = %+v stored=%d", r.Content.Parts, stored(b.e))
	}
	sendText(t, a, b, "only text")
	time.Sleep(200 * time.Millisecond)
	if len(received(b.e)) != 1 || stored(b.e) != 4 {
		t.Fatalf("filtered-out clip was kept or leaked quota: stored=%d", stored(b.e))
	}

	// The receiver's own size limit applies too (the sender allows more).
	updateSettings(t, b.e, func(s *Settings) {
		policy(BackendTailcat, func(p *BackendSyncPolicy) { p.Text = true })(s)
		s.Limits.MaxTotalBytes = 10
	})
	sendText(t, a, b, "this is longer than ten bytes")
	sendText(t, a, b, "short")
	eventually(t, "short one arrives", func() bool {
		rs := received(b.e)
		return len(rs) == 2 && textOf(t, b.e, rs[0]) == "short"
	})
	if len(received(b.e)) != 2 {
		t.Fatal("the over-limit clip was kept")
	}
}

func TestReceivedFilesAreRemovedWithTheirContent(t *testing.T) {
	a, b := pairedPair(t, nil)
	h, _ := a.e.ImportContent(bg, ContentInput{Files: []FileInput{fileIn("f", "bytes")}})
	id, _ := submit(a.e, h)
	waitSend(t, a.e, id)
	r := waitReceived(t, b.e, 1)[0]
	entries, _ := os.ReadDir(b.e.recvDir())
	if len(entries) != 1 {
		t.Fatalf("recv dir = %v", entries)
	}
	_ = b.e.DismissReceived(bg, r.ReceiptID)
	eventually(t, "files removed", func() bool {
		entries, _ := os.ReadDir(b.e.recvDir())
		return len(entries) == 0
	})
}

func TestCloseAndStartupClearTheReceiveDirectory(t *testing.T) {
	a, b := pairedPair(t, nil)
	h, _ := a.e.ImportContent(bg, ContentInput{Files: []FileInput{fileIn("f", "bytes")}})
	id, _ := submit(a.e, h)
	waitSend(t, a.e, id)
	waitReceived(t, b.e, 1)
	if err := b.e.Close(bg); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(b.dir, "inbox", recvDirName)); !os.IsNotExist(err) {
		t.Fatalf("Close left received files: %v", err)
	}
	// A crash leaves them behind; the next start removes them.
	if err := os.MkdirAll(filepath.Join(b.dir, "inbox", recvDirName, "clip-1"), 0o700); err != nil {
		t.Fatal(err)
	}
	opts, _ := newOpts(t, b.dir)
	e2, err := New(bg, opts)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = e2.Close(bg) }()
	if _, err := os.Stat(filepath.Join(b.dir, "inbox", recvDirName)); !os.IsNotExist(err) {
		t.Fatal("startup kept stale received files")
	}
}

// textOfE is textOf for callers without a *testing.T.
func textOfE(e *Engine, r ReceivedContent) string {
	l, err := e.RetainContent(bg, r.Content.ID)
	if err != nil {
		return ""
	}
	defer e.ReleaseContent(bg, l.LeaseID)
	for _, p := range l.Content.Parts {
		if p.MIME == MimeText {
			rd, err := e.ReadContent(bg, l.LeaseID, p.ID)
			if err != nil {
				return ""
			}
			defer rd.Close()
			b, _ := io.ReadAll(rd)
			return string(b)
		}
	}
	return ""
}

// Several sends reuse the one connection, and a path the user paused carries
// nothing in either direction.
func TestConsecutiveSendsAndPausedPath(t *testing.T) {
	a, b := pairedPair(t, nil)
	for i := 0; i < 3; i++ {
		if o := sendText(t, a, b, fmt.Sprint("clip ", i)); o.Outcome != OutcomeSuccess {
			t.Fatalf("send %d = %+v", i, o)
		}
	}
	waitReceived(t, b.e, 3)

	d, err := a.e.GetDevice(bg, string(selfID(b.e)))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := a.e.SetBindingSync(bg, SetBindingSyncRequest{ID: d.Bindings[0].ID, ExpectedRevision: d.Bindings[0].Revision, Enabled: false}); err != nil {
		t.Fatal(err)
	}
	if id, err := submit(a.e, importText(t, a.e, "paused")); err == nil {
		if o := waitSend(t, a.e, id); o.Outcome == OutcomeSuccess {
			t.Fatalf("a paused path was sent to: %+v", o)
		}
	}
	if n := len(received(b.e)); n != 3 {
		t.Fatalf("received %d clips, want 3", n)
	}
}
