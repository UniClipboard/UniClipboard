package builtinsecurity_test

import (
	"bytes"
	"context"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/plugins/builtinclassify"
	"github.com/uniclipboard/unic/internal/plugins/builtinsecurity"
	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/sourceapp"
)

type fakeSyncBackend struct {
	id        string
	submitted bool
}

func (b *fakeSyncBackend) Spec() plugin.Spec {
	return plugin.Spec{
		ID:               b.id,
		Version:          "1.0.0",
		APIVersion:       1,
		Capabilities:     []string{plugin.CapSyncBackend},
		TransportKind:    plugin.TransportObjectStore,
		ExternalChannels: true,
	}
}
func (*fakeSyncBackend) Init(context.Context, plugin.Host, plugin.Config) error { return nil }
func (*fakeSyncBackend) Start(context.Context) error                            { return nil }
func (*fakeSyncBackend) Close(context.Context) error                            { return nil }
func (b *fakeSyncBackend) Submit(context.Context, plugin.SyncView, plugin.SyncHost) (bool, error) {
	b.submitted = true
	return false, nil
}

func evalTempDir(t *testing.T) string {
	t.Helper()
	dir, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	return dir
}

func TestSettingsValidationAndNormalization(t *testing.T) {
	var def builtinsecurity.Settings
	if err := def.Validate(); err != nil {
		t.Fatalf("default settings must validate: %v", err)
	}
	norm := def.Normalized()
	if norm.Mode != builtinsecurity.ModeBlock || norm.ExpireSeconds != 300 || norm.Tag != "sensitive" || norm.MaxScanBytes != 65536 {
		t.Fatalf("unexpected normalized defaults: %+v", norm)
	}
	if !norm.DetectPrivateKeysEnabled() || !norm.DetectTokensAndJWTEnabled() || !norm.DetectPasswordManagersEnabled() {
		t.Fatal("expected built-in detectors enabled by default")
	}

	badCases := []builtinsecurity.Settings{
		{Mode: "invalid"},
		{ExpireSeconds: 5},
		{ExpireSeconds: 90000},
		{Tag: " bad "},
		{MaxScanBytes: 512},
		{CustomPatterns: []string{"[unclosed"}},
		{BlockedApps: []string{"   "}},
	}
	for i, bad := range badCases {
		if err := bad.Validate(); err == nil {
			t.Fatalf("case %d: expected validation error for %+v", i, bad)
		}
	}
}

func TestBuiltinSecurityGuardBlocksSecretsBeforeSyncAndHistory(t *testing.T) {
	ctx := context.Background()
	guard, err := builtinsecurity.New(builtinsecurity.Settings{})
	if err != nil {
		t.Fatal(err)
	}
	if guard.Spec().ProcessorStage != plugin.ProcessorStageGuard {
		t.Fatalf("ProcessorStage = %q, want guard", guard.Spec().ProcessorStage)
	}
	classifier, err := builtinclassify.New(nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	syncBackend := &fakeSyncBackend{id: "thirdparty.sync"}

	var notifications []string
	e, err := engine.New(ctx, engine.Options{
		DataDir:     evalTempDir(t),
		PluginOrder: []string{"thirdparty.sync", builtinclassify.ID, builtinsecurity.ID},
		Plugins: []engine.PluginInstance{
			{Plugin: syncBackend},
			{Plugin: classifier},
			{Plugin: guard, Builtin: true},
		},
		PluginNotify: func(_ engine.PluginID, msg string) {
			notifications = append(notifications, msg)
		},
	})
	if err != nil {
		t.Fatalf("engine.New: %v", err)
	}
	defer func() { _ = e.Close(ctx) }()
	if err := e.Start(ctx); err != nil {
		t.Fatalf("engine.Start: %v", err)
	}

	// 1. Sensitive private key: must NOT persist to history and must NOT sync.
	secretText := "-----BEGIN OPENSSH PRIVATE KEY-----\nSUPER_SECRET_MATERIAL_DO_NOT_LEAK\n-----END OPENSSH PRIVATE KEY-----"
	lease, err := e.ImportContent(ctx, engine.ContentInput{
		Representations: []engine.RepresentationInput{{
			MIME:   engine.MimeText,
			Size:   int64(len(secretText)),
			Reader: strings.NewReader(secretText),
		}},
	})
	if err != nil {
		t.Fatal(err)
	}
	res, err := e.CommitContent(ctx, engine.CommitRequest{RequestID: "req-secret", LeaseID: lease.LeaseID})
	if err != nil {
		t.Fatal(err)
	}
	if res.HistoryID != "" {
		t.Fatalf("expected secret not persisted to history, got history_id=%s", res.HistoryID)
	}
	if res.SyncDisposition != engine.SyncNotRequested {
		t.Fatalf("expected secret sync blocked, got %s", res.SyncDisposition)
	}
	if syncBackend.submitted {
		t.Fatal("sync backend received blocked secret")
	}
	if len(notifications) != 1 || strings.Contains(notifications[0], "SUPER_SECRET_MATERIAL") {
		t.Fatalf("unexpected notification: %v", notifications)
	}

	// 2. Password manager source app: must block even plain text.
	pmText := "my-plain-value"
	pmLease, err := e.ImportContent(ctx, engine.ContentInput{
		SourceApplication: &sourceapp.Application{Platform: "darwin", Identifier: "com.1password.1password", Name: "1Password", Attribution: "foreground"},
		Representations: []engine.RepresentationInput{{
			MIME:   engine.MimeText,
			Size:   int64(len(pmText)),
			Reader: bytes.NewReader([]byte(pmText)),
		}},
	})
	if err != nil {
		t.Fatal(err)
	}
	pmRes, err := e.CommitContent(ctx, engine.CommitRequest{RequestID: "req-pm", LeaseID: pmLease.LeaseID})
	if err != nil {
		t.Fatal(err)
	}
	if pmRes.HistoryID != "" || pmRes.SyncDisposition != engine.SyncNotRequested {
		t.Fatalf("expected password manager item blocked: %+v", pmRes)
	}

	// 3. Normal text: must persist and be classified normally.
	plain := "Hello normal clipboard note"
	plainLease, err := e.ImportContent(ctx, engine.ContentInput{
		Representations: []engine.RepresentationInput{{
			MIME:   engine.MimeText,
			Size:   int64(len(plain)),
			Reader: strings.NewReader(plain),
		}},
	})
	if err != nil {
		t.Fatal(err)
	}
	plainRes, err := e.CommitContent(ctx, engine.CommitRequest{RequestID: "req-plain", LeaseID: plainLease.LeaseID})
	if err != nil {
		t.Fatal(err)
	}
	if plainRes.HistoryID == "" {
		t.Fatal("expected plain text to persist in history")
	}
}

func TestBuiltinSecurityExpireModeAndAllowlist(t *testing.T) {
	ctx := context.Background()
	guard, err := builtinsecurity.New(builtinsecurity.Settings{
		Mode:           builtinsecurity.ModeExpire,
		ExpireSeconds:  120,
		Tag:            "secret-local",
		AllowedApps:    []string{"com.example.trusted"},
		CustomKeywords: []string{"internal-codename"},
	})
	if err != nil {
		t.Fatal(err)
	}
	e, err := engine.New(ctx, engine.Options{
		DataDir: evalTempDir(t),
		Plugins: []engine.PluginInstance{{Plugin: guard, Builtin: true}},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = e.Close(ctx) }()
	if err := e.Start(ctx); err != nil {
		t.Fatal(err)
	}

	// Matched custom keyword in expire mode -> persisted locally with tag & expiry, no sync.
	body := "Project internal-codename launch plan"
	lease, err := e.ImportContent(ctx, engine.ContentInput{
		Representations: []engine.RepresentationInput{{MIME: engine.MimeText, Size: int64(len(body)), Reader: strings.NewReader(body)}},
	})
	if err != nil {
		t.Fatal(err)
	}
	res, err := e.CommitContent(ctx, engine.CommitRequest{RequestID: "req-exp", LeaseID: lease.LeaseID})
	if err != nil {
		t.Fatal(err)
	}
	if res.HistoryID == "" || res.SyncDisposition != engine.SyncNotRequested {
		t.Fatalf("unexpected expire-mode result: %+v", res)
	}
	item, err := e.GetItem(ctx, res.HistoryID)
	if err != nil {
		t.Fatal(err)
	}
	if !slices.Contains(item.Tags, "secret-local") || item.ExpiresAtMs == 0 {
		t.Fatalf("expected tag secret-local and expiry, got tags=%v expires=%d", item.Tags, item.ExpiresAtMs)
	}

	// Allowed app bypasses detection.
	bypassLease, err := e.ImportContent(ctx, engine.ContentInput{
		SourceApplication: &sourceapp.Application{Platform: "darwin", Identifier: "com.example.trusted", Name: "Trusted", Attribution: "foreground"},
		Representations:   []engine.RepresentationInput{{MIME: engine.MimeText, Size: int64(len(body)), Reader: strings.NewReader(body)}},
	})
	if err != nil {
		t.Fatal(err)
	}
	bypassRes, err := e.CommitContent(ctx, engine.CommitRequest{RequestID: "req-allow", LeaseID: bypassLease.LeaseID})
	if err != nil {
		t.Fatal(err)
	}
	bypassItem, err := e.GetItem(ctx, bypassRes.HistoryID)
	if err != nil {
		t.Fatal(err)
	}
	if slices.Contains(bypassItem.Tags, "secret-local") {
		t.Fatalf("allowed app should bypass security guard, got tags=%v", bypassItem.Tags)
	}
}
