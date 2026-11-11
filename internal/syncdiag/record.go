// Package syncdiag defines finite built-in transport observations. It has no
// Engine, transport implementation, exporter or plugin RPC dependency.
package syncdiag

import "time"

// Observer must return promptly. Record and Wake do no disk/network I/O.
// Epoch zero disables collection; a changed epoch discards prior baselines.
type Observer interface {
	SyncEpoch() uint64
	RecordSync(Record)
	WakeSync()
}

type Record struct {
	Epoch       uint64
	Kind        string
	Phase       string
	Direction   string
	Result      string
	Reason      string
	Expected    bool
	Duration    time.Duration
	Path        string
	From        string
	To          string
	ContentKind string
	Declared    int64
	Bytes       int64
}

func SizeClass(n int64) string {
	switch {
	case n < 0:
		return "unknown"
	case n == 0:
		return "empty"
	case n <= 64<<10:
		return "le_64k"
	case n <= 1<<20:
		return "le_1m"
	case n <= 16<<20:
		return "le_16m"
	default:
		return "gt_16m"
	}
}
func Path(v string) bool      { return v == "direct" || v == "derp" || v == "peer_relay" || v == "unknown" }
func KnownPath(v string) bool { return Path(v) && v != "unknown" }
func Reason(v string) bool {
	switch v {
	case "timeout", "cancellation", "unavailable", "invalid_address", "unusable_credentials", "identity_mismatch", "protocol_mismatch", "permission_rejection", "remote_close", "io_failure", "local_stop", "suspension", "revocation", "state_replacement", "unknown":
		return true
	}
	return false
}
func Valid(r Record) bool {
	if r.Epoch == 0 || r.Duration < 0 || r.Bytes < 0 || r.Declared < 0 {
		return false
	}
	direction := r.Direction == "incoming" || r.Direction == "outgoing"
	result := r.Result == "succeeded" || r.Result == "failed" || r.Result == "cancelled"
	switch r.Kind {
	case "connection":
		phase := (r.Phase == "network_start" && r.Direction == "none") || (r.Phase == "dial" && r.Direction == "outgoing") || (r.Phase == "identity_handshake" && direction)
		return phase && (result || ((r.Result == "pending" || r.Result == "rejected") && r.Phase == "identity_handshake")) && Reason(r.Reason)
	case "session":
		return Reason(r.Reason) && r.Expected == (r.Reason == "local_stop" || r.Reason == "suspension" || r.Reason == "revocation" || r.Reason == "state_replacement")
	case "exposure", "pending", "upgrade":
		return true
	case "recovery":
		return r.Result == "recovered" || r.Result == "abandoned" || r.Result == "interrupted"
	case "path":
		return direction && Path(r.Path)
	case "transition":
		return KnownPath(r.From) && KnownPath(r.To) && r.From != r.To
	case "transfer_size":
		return direction && contentKind(r.ContentKind)
	case "transfer":
		return direction && result && Path(r.Path)
	}
	return false
}
func contentKind(v string) bool {
	switch v {
	case "text", "image", "files", "mixed", "unknown":
		return true
	}
	return false
}
