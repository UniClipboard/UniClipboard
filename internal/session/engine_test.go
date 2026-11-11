package session

import (
	"context"
	"errors"
	"fmt"
	"net"
	"strings"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/wire"
	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// blockedNet is a Transport whose Start waits until Close.
type blockedNet struct{ stop chan struct{} }

func (n *blockedNet) Start() error                                   { <-n.stop; return errors.New("closed") }
func (n *blockedNet) Close() error                                   { close(n.stop); return nil }
func (n *blockedNet) Listen(context.Context) (net.Listener, error)   { return nil, errors.New("down") }
func (n *blockedNet) Dial(context.Context, string) (net.Conn, error) { return nil, errors.New("down") }
func (n *blockedNet) Addr() string                                   { return "" }
func (n *blockedNet) Binding(string) ([]byte, error)                 { return nil, errors.New("down") }

// Close must cut a Start that is still waiting for the network short instead
// of waiting for it.
func TestCloseDoesNotWaitForStart(t *testing.T) {
	tr := &blockedNet{stop: make(chan struct{})}
	b := New(Config{Transport: tr})
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()
	_ = b.up(ctx) // Start is still blocked when the context ends
	done := make(chan error, 1)
	go func() { done <- b.Close(context.Background()) }()
	select {
	case err := <-done:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("Close waited for a blocked Start")
	}
}

type introAuthority struct {
	plugin.PeerAuthority
	got chan rpc.Introduce
}

func (a introAuthority) Introduce(_ context.Context, r rpc.Introduce) error { a.got <- r; return nil }

func TestIntroductionsBatchAndExcludeSelfAndRecipient(t *testing.T) {
	b := New(Config{})
	b.self = "self"
	b.st.Introduce = true
	b.st.Peers = append(b.st.Peers, rpc.StatePeer{ID: "recipient", TrustRevision: 1}, rpc.StatePeer{ID: "self", Address: "secret-self"})
	for i := 0; i < 130; i++ {
		b.st.Peers = append(b.st.Peers, rpc.StatePeer{ID: fmt.Sprintf("device-%d", i), Name: "phone", Address: "secret-address"})
	}
	left, right := net.Pipe()
	defer func() { _ = left.Close() }()
	defer func() { _ = right.Close() }()
	s := &session{b: b, peerID: "recipient", trustRev: 1, conn: left, w: wire.NewConn(left), done: make(chan struct{})}
	b.sessions[s.peerID] = s
	done := make(chan error, 1)
	go func() { done <- s.sendIntroductions() }()
	_ = right.SetReadDeadline(time.Now().Add(5 * time.Second))
	w := wire.NewConn(right)
	for _, want := range []int{64, 64, 2} {
		var msg introduction
		if err := readTyped(w, "introduce", &msg); err != nil {
			t.Fatal(err)
		}
		if len(msg.Candidates) != want {
			t.Fatalf("batch size: %d", len(msg.Candidates))
		}
		for _, c := range msg.Candidates {
			if c.ID == "self" || c.ID == "recipient" {
				t.Fatalf("introduced excluded device: %s", c.ID)
			}
		}
	}
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	// Both disabling introductions and losing the recipient's authority must
	// return without writing anything, even when the peer is not reading.
	b.st.Introduce = false
	if err := s.sendIntroductions(); err != nil {
		t.Fatal(err)
	}
	b.st.Introduce = true
	b.st.Peers[0].TrustRevision++
	if err := s.sendIntroductions(); err != nil {
		t.Fatal(err)
	}
}

func TestReceiveIntroductionsUsesLocalSessionAndHonorsSwitch(t *testing.T) {
	for _, enabled := range []bool{false, true} {
		t.Run(fmt.Sprint(enabled), func(t *testing.T) {
			b := New(Config{})
			b.st.Introduce = enabled
			got := make(chan rpc.Introduce, 1)
			b.SetPeerAuthority(introAuthority{got: got})
			left, right := net.Pipe()
			defer func() { _ = left.Close() }()
			defer func() { _ = right.Close() }()
			s := &session{b: b, r: &runState{ctx: context.Background()}, conn: left, w: wire.NewConn(left), connID: "local-connection", token: strings.Repeat("a", 64)}
			done := make(chan error, 1)
			go func() { done <- b.recvLoop(s) }()
			if err := wire.NewConn(right).WriteJSON(introduction{T: "introduce", Candidates: []rpc.Candidate{{ID: "candidate", Name: "phone", Address: "secret"}}}); err != nil {
				t.Fatal(err)
			}
			_ = right.Close()
			err := <-done
			_ = err // This scenario intentionally closes the peer connection.
			select {
			case req := <-got:
				if !enabled || req.ConnectionID != s.connID || req.SessionToken != s.token || len(req.Candidates) != 1 {
					t.Fatalf("wrong forwarding: %+v", req)
				}
			default:
				if enabled {
					t.Fatal("introduction not forwarded")
				}
			}
		})
	}
}
