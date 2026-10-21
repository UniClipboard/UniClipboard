package daemonclient

import "sync/atomic"

// Follow returns the client held by current, replaced first when the connection file now names
// another daemon (a new endpoint or token after a restart that the holder did not record, for
// example when the restart reported a failure but the daemon came up later). The swap is a
// compare-and-swap, so a client stored meanwhile by the owner wins. An empty holder stays empty:
// the owner decides when the first client exists.
func Follow(current *atomic.Pointer[Client]) *Client {
	stored := current.Load()
	if stored == nil {
		return nil
	}
	fresh, err := FromEnv()
	if err != nil || (fresh.BaseURL == stored.BaseURL && fresh.token == stored.token) {
		return stored
	}
	if current.CompareAndSwap(stored, fresh) {
		return fresh
	}
	return current.Load()
}
