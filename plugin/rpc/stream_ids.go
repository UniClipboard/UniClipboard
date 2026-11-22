package rpc

import "sort"

// A compact replay ledger accepts concurrent descriptor arrival without keeping
// one tombstone per completed stream. Normal sequential IDs occupy one range.
type streamIDRange struct{ first, last uint64 }

// acceptPeerStreamIDLocked runs under c.mu after resource admission checks.
func (c *Conn) acceptPeerStreamIDLocked(id uint64) error {
	ranges := c.acceptedPeerStreams
	at := sort.Search(len(ranges), func(i int) bool { return ranges[i].last >= id })
	if at < len(ranges) && ranges[at].first <= id {
		return NewError(CodeInvalidState)
	}
	left := at > 0 && id-ranges[at-1].last == 2
	right := at < len(ranges) && ranges[at].first-id == 2
	switch {
	case left && right:
		ranges[at-1].last = ranges[at].last
		copy(ranges[at:], ranges[at+1:])
		ranges = ranges[:len(ranges)-1]
	case left:
		ranges[at-1].last = id
	case right:
		ranges[at].first = id
	default:
		// Bound hostile sparse IDs using the existing negotiated work quotas.
		if len(ranges) >= c.lim.MaxStreams+c.lim.MaxRequests+1 {
			return NewError(CodeExhausted)
		}
		ranges = append(ranges, streamIDRange{})
		copy(ranges[at+1:], ranges[at:])
		ranges[at] = streamIDRange{first: id, last: id}
	}
	c.acceptedPeerStreams = ranges
	return nil
}
