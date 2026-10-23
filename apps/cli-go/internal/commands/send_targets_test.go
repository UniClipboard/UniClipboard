package commands

import (
	"errors"
	"testing"
	"testing/synctest"
	"time"
)

func TestSendVirtualTimeTargets(t *testing.T) {
	tests := []struct {
		name         string
		requested    []string
		peers        []peerSnapshot
		onlineAt     time.Duration
		queryErrorAt int
		budget       time.Duration
		elapsed      time.Duration
		reason       string
	}{
		{name: "V01_online", requested: []string{"a"}, peers: []peerSnapshot{{PeerID: "a", Connected: true}}, budget: 60 * time.Second},
		{name: "V02_late", requested: []string{"a"}, peers: []peerSnapshot{{PeerID: "a"}}, onlineAt: 4 * time.Second, budget: 60 * time.Second, elapsed: 4 * time.Second},
		{name: "V03_offline_60s", requested: []string{"a"}, peers: []peerSnapshot{{PeerID: "a"}}, budget: 60 * time.Second, elapsed: 60 * time.Second, reason: "a (offline)"},
		{name: "V04_all_required", requested: []string{"a", "b"}, peers: []peerSnapshot{{PeerID: "a", Connected: true}, {PeerID: "b"}}, onlineAt: 4 * time.Second, budget: 60 * time.Second, elapsed: 4 * time.Second},
		{name: "V04_partial_timeout", requested: []string{"a", "b"}, peers: []peerSnapshot{{PeerID: "a", Connected: true}, {PeerID: "b"}}, budget: time.Second, elapsed: time.Second, reason: "b (offline)"},
		{name: "V05_unknown_order", requested: []string{"b", "a"}, budget: time.Second, elapsed: time.Second, reason: "b (unknown device), a (unknown device)"},
		{name: "V06_any_paired", peers: []peerSnapshot{{PeerID: "a", IsPaired: true, Connected: true}, {PeerID: "b", IsPaired: true}}, budget: 60 * time.Second},
		{name: "V07_all_offline", peers: []peerSnapshot{{PeerID: "a", IsPaired: true}, {PeerID: "b", IsPaired: true}}, budget: time.Second, elapsed: time.Second, reason: "none of 2 paired device(s) is connected"},
		{name: "V08_no_paired", budget: 60 * time.Second},
		{name: "V08_only_unpaired", peers: []peerSnapshot{{PeerID: "a", Connected: true}}, budget: 60 * time.Second},
		{name: "V08_ignore_unpaired_online", peers: []peerSnapshot{{PeerID: "a", Connected: true}, {PeerID: "b", IsPaired: true}}, budget: time.Second, elapsed: time.Second, reason: "none of 1 paired device(s) is connected"},
		{name: "V09_initial_query_error", requested: []string{"a"}, queryErrorAt: 1, budget: 60 * time.Second},
		{name: "V09_later_query_error", requested: []string{"a"}, queryErrorAt: 3, budget: 60 * time.Second, elapsed: time.Second},
		{name: "V12_ready_at_deadline", requested: []string{"a"}, peers: []peerSnapshot{{PeerID: "a"}}, onlineAt: time.Second, budget: time.Second, elapsed: time.Second},
		{name: "V12_query_error_at_deadline", requested: []string{"a"}, queryErrorAt: 3, budget: time.Second, elapsed: time.Second},
		{name: "V13_partial_interval", requested: []string{"a"}, budget: 250 * time.Millisecond, elapsed: 250 * time.Millisecond, reason: "a (unknown device)"},
		{name: "V12_expired_ready", requested: []string{"a"}, peers: []peerSnapshot{{PeerID: "a", Connected: true}}, budget: -time.Second},
		{name: "V12_expired_query_error", requested: []string{"a"}, queryErrorAt: 1, budget: -time.Second},
		{name: "V13_expired", requested: []string{"a"}, budget: -time.Second, reason: "a (unknown device)"},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				start := time.Now()
				queries, notifications := 0, 0
				reason := pollSendTargets(tc.requested, start.Add(tc.budget), func() ([]peerSnapshot, error) {
					queries++
					if tc.queryErrorAt > 0 && queries == tc.queryErrorAt {
						return nil, errors.New("query failed")
					}
					peers := append([]peerSnapshot(nil), tc.peers...)
					if tc.onlineAt > 0 && time.Since(start) >= tc.onlineAt {
						for i := range peers {
							peers[i].Connected = true
						}
					}
					return peers, nil
				}, func() { notifications++ })
				if reason != tc.reason || time.Since(start) != tc.elapsed {
					t.Fatalf("reason=%q elapsed=%v, want %q %v", reason, time.Since(start), tc.reason, tc.elapsed)
				}
				wantQueries := int(tc.elapsed/targetPollInterval) + 1
				if tc.elapsed%targetPollInterval != 0 {
					wantQueries++
				}
				if queries != wantQueries {
					t.Fatalf("queries=%d, want %d", queries, wantQueries)
				}
				wantNotifications := queries - 1
				if reason != "" {
					wantNotifications = queries
				}
				if notifications != wantNotifications {
					t.Fatalf("waiting notifications=%d, want %d", notifications, wantNotifications)
				}
			})
		})
	}
}
