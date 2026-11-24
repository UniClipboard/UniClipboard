package syncpath

import (
	"context"
	"encoding/json"
	"io"
	"net"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"

	"github.com/tailscale/tailcat"
	"tailscale.com/derp/derpserver"
	"tailscale.com/tailcfg"
	"tailscale.com/types/key"

	"github.com/uniclipboard/unic/internal/tcnet"
)

// A restarted listener can keep the same address but lose registration of a
// dialer that stayed up. Only that dialer can restore its registration.
func TestTailcatReconnectAfterListenerRestart(t *testing.T) {
	if os.Getenv("UNIC_SYNC_PATH_E2E") != "1" {
		t.Skip("set UNIC_SYNC_PATH_E2E=1")
	}
	if os.Getenv("IN_TS_TEST") != "true" || os.Getenv("TS_DEBUG_NEVER_DIRECT_UDP") != "true" {
		t.Fatal("set IN_TS_TEST=true and TS_DEBUG_NEVER_DIRECT_UDP=true")
	}
	silent := func(string, ...any) {}
	derp := derpserver.New(key.NewNode(), silent)
	relay := httptest.NewTLSServer(derpserver.Handler(derp))
	t.Cleanup(relay.Close)
	t.Cleanup(func() { _ = derp.Close() })
	region := &tailcfg.DERPRegion{RegionID: 1, Nodes: []*tailcfg.DERPNode{{
		Name: "fixture", RegionID: 1, HostName: "127.0.0.1", IPv4: "127.0.0.1",
		IPv6: "none", DERPPort: relay.Listener.Addr().(*net.TCPAddr).Port, STUNPort: -1, InsecureForTests: true,
	}}}

	home := t.TempDir()
	sk, ck, psk := key.NewNode(), key.NewNode(), tailcat.NewPresharedKey()
	ci := tailcat.ConnInfo{
		ServerPublic: tailcat.NodePublic{NodePublic: sk.Public()},
		PresharedKey: psk, Region: []*tailcfg.DERPRegion{region},
	}
	creds, err := json.Marshal(map[string]any{
		"version": 1, "server_key": sk, "client_key": ck, "psk": psk, "addr": ci.Addr(),
	})
	must(t, err)
	must(t, os.WriteFile(filepath.Join(home, "tailcat.json"), creds, 0600))

	dialer, err := tcnet.New(t.TempDir(), silent)
	must(t, err)
	startListener := func() (*exec.Cmd, string) {
		t.Helper()
		cmd := exec.Command(os.Args[0], "-test.run=^TestTailcatReconnectListenerProcess$")
		cmd.Env = append(os.Environ(), "UNIC_TAILCAT_RECONNECT_LISTENER="+home)
		must(t, cmd.Start())
		t.Cleanup(func() {
			if cmd.ProcessState == nil {
				_ = cmd.Process.Kill()
				_ = cmd.Wait()
			}
		})
		ctx, cancel := context.WithTimeout(context.Background(), 12*time.Second)
		defer cancel()
		until(t, ctx, func() bool { return derp.IsClientConnectedForTest(sk.Public()) })
		body, err := os.ReadFile(filepath.Join(home, "tailcat.json"))
		must(t, err)
		var saved struct{ Addr string }
		must(t, json.Unmarshal(body, &saved))
		return cmd, saved.Addr
	}
	exchange := func(addr, want string) {
		t.Helper()
		ctx, cancel := context.WithTimeout(context.Background(), 12*time.Second)
		defer cancel()
		c, err := dialer.Dial(ctx, addr)
		if err != nil {
			t.Fatalf("%s: dial failed: %T", want, err)
		}
		defer func() {
			if err := c.Close(); err != nil {
				t.Errorf("close fixture connection: %T", err)
			}
		}()
		deadline, _ := ctx.Deadline()
		must(t, c.SetDeadline(deadline))
		_, err = io.WriteString(c, want)
		must(t, err)
		got := make([]byte, len(want))
		_, err = io.ReadFull(c, got)
		must(t, err)
		if string(got) != want {
			t.Fatal("payload mismatch")
		}
		t.Logf("delivered %s", want)
	}

	first, addr := startListener()
	exchange(addr, "before restart")
	must(t, first.Process.Kill())
	_ = first.Wait()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	until(t, ctx, func() bool { return !derp.IsClientConnectedForTest(sk.Public()) })
	cancel()
	outage, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	conn, err := dialer.Dial(outage, addr)
	if err == nil {
		_ = conn.Close()
		t.Fatal("dial succeeded while the listener was down")
	}
	if outage.Err() == nil {
		t.Fatalf("dial failed before the outage deadline: %T", err)
	}
	t.Log("dial timed out while listener was down")
	_, reopenedAddr := startListener()
	if reopenedAddr != addr {
		t.Fatal("listener address changed on restart")
	}
	exchange(addr, "after restart")
}

func TestTailcatReconnectListenerProcess(t *testing.T) {
	home := os.Getenv("UNIC_TAILCAT_RECONNECT_LISTENER")
	if home == "" {
		t.Skip("reconnect fixture subprocess")
	}
	tr, err := tcnet.New(home, func(string, ...any) {})
	must(t, err)
	must(t, tr.Start())
	ln, err := tr.Listen(context.Background())
	must(t, err)
	for {
		c, err := ln.Accept()
		must(t, err)
		_, _ = io.Copy(c, c)
		_ = c.Close()
	}
}
