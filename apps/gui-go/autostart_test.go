package main

import (
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/wailsapp/wails/v3/pkg/application"
)

// fakeOS stands in for app.Autostart (the Wails manager) and records the calls the adapter makes.
type fakeOS struct {
	status     application.AutostartStatus
	enableErr  error
	disableErr error
	calls      []string
	options    []application.AutostartOptions
	onEnable   func()
}

func (f *fakeOS) EnableWithOptions(o application.AutostartOptions) error {
	f.calls = append(f.calls, "enable")
	f.options = append(f.options, o)
	if f.onEnable != nil {
		f.onEnable()
	}
	return f.enableErr
}
func (f *fakeOS) Disable() error {
	f.calls = append(f.calls, "disable")
	return f.disableErr
}
func (f *fakeOS) Status() (application.AutostartStatus, error) { return f.status, nil }

const bundledExe = "/Applications/UniClipboard.app/Contents/MacOS/gui-go"

func policy(t *testing.T, profile, exe string, allowProfileBundle bool) loginItemPolicy {
	t.Helper()
	return loginItemPolicy{ProductName: "UniClipboard", Profile: profile, Executable: exe, Home: t.TempDir(), AllowProfileBundle: allowProfileBundle}
}

func writeAgent(t *testing.T, home, name, exe string) string {
	t.Helper()
	dir := filepath.Join(home, "Library", "LaunchAgents")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, name+".plist")
	body := `<plist><dict><key>Label</key><string>` + name + `</string><key>ProgramArguments</key><array><string>` + exe + `</string><string>--autostart</string></array></dict></plist>`
	if err := os.WriteFile(path, []byte(body), 0o644); err != nil {
		t.Fatal(err)
	}
	return path
}

func exists(path string) bool { _, err := os.Stat(path); return err == nil }

func TestRunningFromAppBundle(t *testing.T) {
	cases := map[string]bool{
		bundledExe:                          true,
		"/tmp/x/Foo.app/Contents/MacOS/bar": true,
		"/tmp/x/gui-go":                     false,
		"/tmp/x/Foo.app/Contents/Helpers/b": false,
		"/tmp/Foo/Contents/MacOS/bar":       false,
	}
	for exe, want := range cases {
		if got := runningFromAppBundle(exe); got != want {
			t.Errorf("runningFromAppBundle(%q) = %v, want %v", exe, got, want)
		}
	}
}

// Failure mode 1: Identifier is ignored on the SMAppService path, so a named profile launched from a bundle
// would register or unregister the whole bundle's login item.
func TestNamedProfileInBundleNeverReachesTheOS(t *testing.T) {
	p := policy(t, "dev", bundledExe, false)
	for _, enabled := range []bool{true, false} {
		fake := &fakeOS{}
		err := p.apply(fake, enabled, false)
		if !errors.Is(err, errProfileLoginItem) {
			t.Fatalf("enabled=%v: want errProfileLoginItem, got %v", enabled, err)
		}
		if len(fake.calls) != 0 {
			t.Fatalf("enabled=%v: OS was touched: %v", enabled, fake.calls)
		}
	}
}

func TestNamedProfileInBundleAllowedOnlyForTheTestBuild(t *testing.T) {
	p := policy(t, "dev", bundledExe, true)
	fake := &fakeOS{}
	if err := p.apply(fake, true, false); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(fake.calls, []string{"enable"}) {
		t.Fatalf("calls = %v", fake.calls)
	}
}

func TestEnableOptionsCarryTheLoginItemIdentity(t *testing.T) {
	primary := &fakeOS{}
	if err := policy(t, "", bundledExe, false).apply(primary, true, false); err != nil {
		t.Fatal(err)
	}
	want := application.AutostartOptions{Identifier: "UniClipboard", Arguments: []string{"--autostart"}}
	if !reflect.DeepEqual(primary.options, []application.AutostartOptions{want}) {
		t.Fatalf("primary options = %+v", primary.options)
	}
	dev := &fakeOS{}
	if err := policy(t, "dev", "/tmp/x/gui-go", false).apply(dev, true, false); err != nil {
		t.Fatal(err)
	}
	want = application.AutostartOptions{Identifier: "UniClipboard-dev", Arguments: []string{"--autostart"}}
	if !reflect.DeepEqual(dev.options, []application.AutostartOptions{want}) {
		t.Fatalf("profile options = %+v", dev.options)
	}
}

// Failure mode 8: repeated user-initiated enable always rewrites (idempotent), even when already enabled.
func TestUserEnableIsRepeatable(t *testing.T) {
	fake := &fakeOS{status: application.AutostartStatus{Enabled: true}}
	p := policy(t, "", bundledExe, false)
	for i := 0; i < 3; i++ {
		if err := p.apply(fake, true, false); err != nil {
			t.Fatal(err)
		}
	}
	if !reflect.DeepEqual(fake.calls, []string{"enable", "enable", "enable"}) {
		t.Fatalf("calls = %v", fake.calls)
	}
}

// Failure mode 5: IsEnabled maps "requires approval" to false, so disable must not be gated on it.
func TestDisableIsUnconditional(t *testing.T) {
	fake := &fakeOS{status: application.AutostartStatus{Enabled: false}}
	if err := policy(t, "", bundledExe, false).apply(fake, false, false); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(fake.calls, []string{"disable"}) {
		t.Fatalf("calls = %v", fake.calls)
	}
}

// Failure mode 6: reconcile at startup must not re-register (re-bootstrap) an entry that is already healthy.
func TestStartupReconcileSkipsAHealthyRegistration(t *testing.T) {
	fake := &fakeOS{status: application.AutostartStatus{Enabled: true, Strategy: application.AutostartStrategySMAppService}}
	if err := policy(t, "", bundledExe, false).apply(fake, true, true); err != nil {
		t.Fatal(err)
	}
	if len(fake.calls) != 0 {
		t.Fatalf("calls = %v", fake.calls)
	}
	fake = &fakeOS{}
	if err := policy(t, "", bundledExe, false).apply(fake, true, true); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(fake.calls, []string{"enable"}) {
		t.Fatalf("not-registered calls = %v", fake.calls)
	}
}

func TestOSFailureIsReturned(t *testing.T) {
	boom := errors.New("boom")
	if err := policy(t, "", bundledExe, false).apply(&fakeOS{enableErr: boom}, true, false); !errors.Is(err, boom) {
		t.Fatalf("enable err = %v", err)
	}
	if err := policy(t, "", bundledExe, false).apply(&fakeOS{disableErr: boom}, false, false); !errors.Is(err, boom) {
		t.Fatalf("disable err = %v", err)
	}
}

// Failure mode 4: a Tauri-era entry named like the login item but pointing at another executable is removed
// (otherwise it launches the app a second time); an entry for the current executable and unrelated entries stay.
func TestLegacyLoginItemSweep(t *testing.T) {
	const tauriExe = "/Applications/UniClipboard.app/Contents/MacOS/uniclipboard"
	p := policy(t, "", bundledExe, false)
	legacy := writeAgent(t, p.Home, "UniClipboard", tauriExe)
	other := writeAgent(t, p.Home, "com.example.other", tauriExe)
	devOnly := writeAgent(t, p.Home, "UniClipboard-dev", tauriExe)
	fake := &fakeOS{onEnable: func() {
		if exists(legacy) {
			t.Error("legacy entry still present when the new one registered")
		}
	}}
	if err := p.apply(fake, true, false); err != nil {
		t.Fatal(err)
	}
	if exists(legacy) {
		t.Fatal("legacy entry survived")
	}
	if !exists(other) || !exists(devOnly) {
		t.Fatal("the sweep touched an entry that is not this login item's")
	}
	// Disabling sweeps too: a leftover legacy entry would keep launching the app.
	legacy = writeAgent(t, p.Home, "UniClipboard", tauriExe)
	if err := p.apply(&fakeOS{}, false, false); err != nil || exists(legacy) {
		t.Fatalf("disable did not sweep: err=%v exists=%v", err, exists(legacy))
	}
}

func TestSweepKeepsTheCurrentExecutableEntry(t *testing.T) {
	p := policy(t, "", bundledExe, false)
	current := writeAgent(t, p.Home, "UniClipboard", bundledExe)
	if err := p.apply(&fakeOS{status: application.AutostartStatus{Enabled: true}}, true, true); err != nil {
		t.Fatal(err)
	}
	if !exists(current) {
		t.Fatal("an entry for the running executable was removed")
	}
}

// The profile-suffixed name is the only one a named profile may sweep: the primary login item is never read.
func TestProfileSweepNeverTouchesThePrimaryEntry(t *testing.T) {
	p := policy(t, "dev", "/tmp/x/gui-go", false)
	primary := writeAgent(t, p.Home, "UniClipboard", "/Applications/UniClipboard.app/Contents/MacOS/uniclipboard")
	stale := writeAgent(t, p.Home, "UniClipboard-dev", "/old/gui-go")
	if err := p.apply(&fakeOS{}, true, false); err != nil {
		t.Fatal(err)
	}
	if !exists(primary) {
		t.Fatal("the primary login item was removed by a profile instance")
	}
	if exists(stale) {
		t.Fatal("the profile's own stale entry survived")
	}
}

type fakeStore struct {
	value    bool
	failSet  bool
	setCalls []bool
}

func (s *fakeStore) get() (bool, error) { return s.value, nil }
func (s *fakeStore) set(v bool) error {
	s.setCalls = append(s.setCalls, v)
	if s.failSet {
		return errors.New("daemon down")
	}
	s.value = v
	return nil
}

// Failure mode 8: the preference is persisted first and rolled back when the OS change fails.
func TestPreferenceRollsBackWhenTheOSFails(t *testing.T) {
	store := &fakeStore{value: true}
	err := applyAutoStart(store, policy(t, "", bundledExe, false), &fakeOS{disableErr: errors.New("read-only")}, false)
	if err == nil {
		t.Fatal("expected an error")
	}
	if !store.value || !reflect.DeepEqual(store.setCalls, []bool{false, true}) {
		t.Fatalf("value=%v sets=%v", store.value, store.setCalls)
	}
}

func TestPreferenceStaysWhenTheOSSucceeds(t *testing.T) {
	store := &fakeStore{}
	if err := applyAutoStart(store, policy(t, "", bundledExe, false), &fakeOS{}, true); err != nil || !store.value {
		t.Fatalf("err=%v value=%v", err, store.value)
	}
}

// A refused profile instance must not leave a preference the OS never reached.
func TestRefusedProfileInstanceRollsBack(t *testing.T) {
	store := &fakeStore{}
	err := applyAutoStart(store, policy(t, "dev", bundledExe, false), &fakeOS{}, true)
	if err == nil || !strings.Contains(err.Error(), errProfileLoginItem.Error()) || store.value {
		t.Fatalf("err=%v value=%v", err, store.value)
	}
}
