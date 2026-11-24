package engine

import (
	"bytes"
	"context"
	"io"
	"log/slog"
	"strings"
	"sync"
	"testing"

	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

type capturedLog struct {
	level slog.Level
	msg   string
	attrs map[string]string
}

type captureHandler struct {
	mu      sync.Mutex
	records []capturedLog
}

func (h *captureHandler) Enabled(context.Context, slog.Level) bool { return true }
func (h *captureHandler) WithAttrs([]slog.Attr) slog.Handler       { return h }
func (h *captureHandler) WithGroup(string) slog.Handler            { return h }
func (h *captureHandler) Handle(_ context.Context, r slog.Record) error {
	attrs := map[string]string{}
	r.Attrs(func(a slog.Attr) bool {
		attrs[a.Key] = a.Value.String()
		return true
	})
	h.mu.Lock()
	h.records = append(h.records, capturedLog{level: r.Level, msg: r.Message, attrs: attrs})
	h.mu.Unlock()
	return nil
}

func (h *captureHandler) popPipelineFailed() []capturedLog {
	h.mu.Lock()
	defer h.mu.Unlock()
	var out []capturedLog
	for _, r := range h.records {
		if r.msg == "pipeline.failed" {
			out = append(out, r)
		}
	}
	h.records = nil
	return out
}

type testTimeoutErr struct{}

func (testTimeoutErr) Error() string   { return "i/o timeout" }
func (testTimeoutErr) Timeout() bool   { return true }
func (testTimeoutErr) Temporary() bool { return true }

type prefixErrReader struct {
	prefix []byte
	err    error
}

func (r *prefixErrReader) Read(p []byte) (int, error) {
	if len(r.prefix) > 0 {
		n := copy(p, r.prefix)
		r.prefix = r.prefix[n:]
		return n, nil
	}
	return 0, r.err
}

type readerBodies struct {
	rs []io.Reader
	i  int
}

func (b *readerBodies) Next(context.Context) (int, io.ReadCloser, error) {
	if b.i >= len(b.rs) {
		return 0, nil, io.EOF
	}
	idx := b.i
	b.i++
	return idx, io.NopCloser(b.rs[idx]), nil
}

type failingSendPeer struct {
	fakePeer
	sendErr error
}

func (f *failingSendPeer) Send(ctx context.Context, s rpc.Send, src plugin.SendSource) error {
	f.sends.Add(1)
	if _, err := src.Authorize(ctx, rpc.Authorize{
		DispatchID: s.DispatchID, ContentHandle: s.ContentHandle,
		ConnectionID: s.ConnectionID, SessionToken: s.SessionToken,
	}); err != nil {
		return err
	}
	return f.sendErr
}

func TestReceiveFailureDiagnosticsAndStructuredLogs(t *testing.T) {
	var diagMu sync.Mutex
	var diags []DiagnosticRecord
	popDiags := func() []DiagnosticRecord {
		diagMu.Lock()
		defer diagMu.Unlock()
		out := append([]DiagnosticRecord(nil), diags...)
		diags = nil
		return out
	}
	logs := &captureHandler{}

	fa, fb := &fakePeer{}, &fakePeer{}
	na := newContentNode(t, func(o *Options) { o.Plugins = []PluginInstance{{Plugin: fa}} })
	nb := newContentNode(t, func(o *Options) {
		o.Plugins = []PluginInstance{{Plugin: fb}}
		o.Logger = slog.New(logs)
		o.Diagnostics = func(_ context.Context, r DiagnosticRecord) {
			diagMu.Lock()
			diags = append(diags, r)
			diagMu.Unlock()
		}
		o.Settings = DefaultSettings()
		o.Settings.Limits.MaxFileBytes = 10
	})
	bind := []byte("diag")
	if _, ea, _, eb := runHandshake(t, fa, fb, "diag-init", nil, bind, bind); ea != nil || eb != nil {
		t.Fatal(ea, eb)
	}
	idA := na.e.ident.DeviceID()
	da, _ := nb.e.st.Device(idA)
	if _, err := nb.e.st.TrustDevice(idA, da.Revision); err != nil {
		t.Fatal(err)
	}
	bd, err := nb.e.st.InstanceBinding(idA, "test.peer")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := nb.e.st.AuthorizeBinding(bd.ID, bd.Revision); err != nil {
		t.Fatal(err)
	}
	nb.e.syncPeerStates()
	start(t, nb)

	_, _, dB, eB := runHandshake(t, fa, fb, "diag-conn", nil, bind, bind)
	if eB != nil || dB.Decision != "allow" || dB.SessionToken == nil {
		t.Fatalf("handshake: %+v %v", dB, eB)
	}
	tok := *dB.SessionToken

	assertDiagAndLog := func(t *testing.T, wantResult DiagnosticResult, wantCode Code) {
		t.Helper()
		ds := popDiags()
		var gotDiag *DiagnosticRecord
		for i := range ds {
			if ds[i].Stage == "receive" {
				gotDiag = &ds[i]
			}
		}
		if gotDiag == nil || gotDiag.Result != wantResult || gotDiag.Code != wantCode || gotDiag.ContentKind != "files" {
			t.Fatalf("receive diagnostic = %+v (all=%+v), want result=%q code=%q kind=files", gotDiag, ds, wantResult, wantCode)
		}
		ls := logs.popPipelineFailed()
		if len(ls) != 1 {
			t.Fatalf("pipeline.failed log count = %d (%+v), want 1", len(ls), ls)
		}
		l := ls[0]
		if l.level != slog.LevelWarn || l.attrs["stage"] != "receive" || l.attrs["result"] != string(wantResult) ||
			l.attrs["backend"] != "external" || l.attrs["content_kind"] != "files" || l.attrs["code"] != string(wantCode) {
			t.Fatalf("pipeline.failed log = %+v", l)
		}
		if _, hasReason := l.attrs["reason"]; hasReason {
			t.Fatalf("unapproved reason attribute leaked: %+v", l.attrs)
		}
	}

	openFile := func(wireID string, size uint64) rpc.ReceiveGrant {
		t.Helper()
		g, err := fb.recv.OpenReceive(bg, rpc.ReceiveOpen{
			ConnectionID: "diag-conn", SessionToken: tok,
			Clip: rpc.Clip{
				WireID: wireID, Origin: idA, Hash: strings.Repeat("a", 64),
				Parts: []rpc.ClipPart{{Index: 0, Kind: "file", Name: "large.bin", Size: rpc.U64(size)}},
			},
		})
		if err != nil {
			t.Fatalf("OpenReceive: %v", err)
		}
		return g
	}

	t.Run("read_timeout", func(t *testing.T) {
		popDiags()
		logs.popPipelineFailed()
		g := openFile("w-timeout", 8)
		if g.Decision != rpc.DecisionAccept {
			t.Fatalf("decision = %q", g.Decision)
		}
		_, err := fb.recv.Receive(bg, *g.ReceiveID, &readerBodies{rs: []io.Reader{
			&prefixErrReader{prefix: []byte("abcd"), err: testTimeoutErr{}},
		}})
		if err == nil {
			t.Fatal("expected error")
		}
		assertDiagAndLog(t, "failed", CodeTimeout)
	})

	t.Run("unexpected_eof", func(t *testing.T) {
		popDiags()
		logs.popPipelineFailed()
		g := openFile("w-eof", 8)
		if g.Decision != rpc.DecisionAccept {
			t.Fatalf("decision = %q", g.Decision)
		}
		_, err := fb.recv.Receive(bg, *g.ReceiveID, &readerBodies{rs: []io.Reader{
			&prefixErrReader{prefix: []byte("abcd"), err: io.ErrUnexpectedEOF},
		}})
		if err == nil {
			t.Fatal("expected error")
		}
		assertDiagAndLog(t, "failed", CodeUnavailable)
	})

	t.Run("hash_mismatch", func(t *testing.T) {
		popDiags()
		logs.popPipelineFailed()
		g := openFile("w-hash", 4)
		if g.Decision != rpc.DecisionAccept {
			t.Fatalf("decision = %q", g.Decision)
		}
		_, err := fb.recv.Receive(bg, *g.ReceiveID, &readerBodies{rs: []io.Reader{
			bytes.NewReader([]byte("abcd")),
		}})
		if errCode(err) != rpc.CodeInvalidState {
			t.Fatalf("err = %v, want invalid_state", err)
		}
		assertDiagAndLog(t, "failed", CodeInvalidState)
	})

	t.Run("limit_exceeded", func(t *testing.T) {
		popDiags()
		logs.popPipelineFailed()
		g := openFile("w-limit", 11)
		if g.Decision != rpc.DecisionDiscard {
			t.Fatalf("decision = %q, want discard", g.Decision)
		}
		assertDiagAndLog(t, "rejected", CodeLimitExceeded)
	})
}

func TestSendFailureDiagnosticsAndStructuredLogs(t *testing.T) {
	for _, tc := range []struct {
		name      string
		sendErr   error
		wantCode  Code
		retryable bool
	}{
		{name: "timeout", sendErr: rpc.NewError(rpc.CodeTimeout), wantCode: CodeTimeout, retryable: true},
		{name: "unavailable", sendErr: rpc.NewError(rpc.CodeUnavailable), wantCode: CodeUnavailable, retryable: true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			var diagMu sync.Mutex
			var diags []DiagnosticRecord
			logs := &captureHandler{}

			fa := &failingSendPeer{sendErr: tc.sendErr}
			fb := &fakePeer{}
			na := newContentNode(t, func(o *Options) {
				o.Plugins = []PluginInstance{{Plugin: fa}}
				o.Logger = slog.New(logs)
				o.Diagnostics = func(_ context.Context, r DiagnosticRecord) {
					diagMu.Lock()
					diags = append(diags, r)
					diagMu.Unlock()
				}
			})
			nb := newContentNode(t, func(o *Options) { o.Plugins = []PluginInstance{{Plugin: fb}} })
			bind := []byte("senddiag")
			if _, ea, _, eb := runHandshake(t, &fa.fakePeer, fb, "s1", nil, bind, bind); ea != nil || eb != nil {
				t.Fatal(ea, eb)
			}
			idB := nb.e.ident.DeviceID()
			d, _ := na.e.st.Device(idB)
			if _, err := na.e.st.TrustDevice(idB, d.Revision); err != nil {
				t.Fatal(err)
			}
			bd, err := na.e.st.InstanceBinding(idB, "test.peer")
			if err != nil {
				t.Fatal(err)
			}
			if _, err := na.e.st.AuthorizeBinding(bd.ID, bd.Revision); err != nil {
				t.Fatal(err)
			}
			na.e.syncPeerStates()
			start(t, na)

			dA, eA, _, _ := runHandshake(t, &fa.fakePeer, fb, "s2", nil, bind, bind)
			if eA != nil || dA.Decision != "allow" || dA.SessionToken == nil {
				t.Fatalf("handshake: %+v %v", dA, eA)
			}
			if err := fa.auth.PeerEvent(bg, rpc.PeerEvent{ConnectionID: "s2", SessionToken: *dA.SessionToken, Sequence: 1, State: "connected", RemoteAccepted: true}); err != nil {
				t.Fatal(err)
			}

			h, err := na.e.ImportContent(bg, ContentInput{Files: []FileInput{fileIn("payload.bin", "123456")}})
			if err != nil {
				t.Fatal(err)
			}
			opID, err := submit(na.e, h)
			if err != nil {
				t.Fatal(err)
			}
			op := waitSend(t, na.e, opID)
			if len(op.Targets) != 1 || op.Targets[0].State != TargetFailed || op.Targets[0].Error == nil {
				t.Fatalf("op = %+v", op)
			}
			if op.Targets[0].Error.Code != tc.wantCode || op.Targets[0].Error.Retryable != tc.retryable {
				t.Fatalf("target err = %+v, want code=%q retryable=%v", op.Targets[0].Error, tc.wantCode, tc.retryable)
			}

			diagMu.Lock()
			var gotSend *DiagnosticRecord
			for i := range diags {
				if diags[i].Stage == "send" {
					gotSend = &diags[i]
				}
			}
			diagMu.Unlock()
			if gotSend == nil || gotSend.Result != "failed" || gotSend.Code != tc.wantCode || gotSend.ContentKind != "files" {
				t.Fatalf("send diagnostic = %+v, want failed/%q/files", gotSend, tc.wantCode)
			}

			ls := logs.popPipelineFailed()
			if len(ls) != 1 {
				t.Fatalf("pipeline.failed logs = %+v, want 1", ls)
			}
			l := ls[0]
			if l.level != slog.LevelWarn || l.attrs["stage"] != "send" || l.attrs["result"] != "failed" ||
				l.attrs["backend"] != "external" || l.attrs["content_kind"] != "files" || l.attrs["code"] != string(tc.wantCode) {
				t.Fatalf("pipeline.failed log = %+v", l)
			}
		})
	}
}
