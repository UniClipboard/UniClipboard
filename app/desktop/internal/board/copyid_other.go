//go:build (!darwin && !windows) || (darwin && !cgo)

package board

// mygo exposes no portable clipboard generation. Identical repeated copies
// cannot be distinguished on this adapter; never scan to fabricate a marker.
func clipboardCopyID() string { return "" }
