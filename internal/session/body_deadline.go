package session

import (
	"net"
	"time"
)

// bodyDeadlineConn bounds a stalled framed body without timing out idle peers
// or imposing a total transfer deadline. The single wire reader owns body.
// Reads and writes use separate flags and deadlines; control writes keep their own budget.
// A bulk body may encounter longer network gaps than a small control event.
const bodyIdleTimeout = 30 * time.Second

type bodyDeadlineConn struct {
	net.Conn
	body        bool
	writingBody bool
}

func (c *bodyDeadlineConn) Read(p []byte) (int, error) {
	if c.body {
		if err := c.SetReadDeadline(time.Now().Add(bodyIdleTimeout)); err != nil {
			return 0, err
		}
		defer func() { _ = c.SetReadDeadline(time.Time{}) }()
	}
	return c.Conn.Read(p)
}

// The single framed writer owns writingBody while holding the session writer lock.
func (c *bodyDeadlineConn) Write(p []byte) (int, error) {
	if c.writingBody {
		if err := c.SetWriteDeadline(time.Now().Add(bodyIdleTimeout)); err != nil {
			return 0, err
		}
		defer func() { _ = c.SetWriteDeadline(time.Time{}) }()
	}
	return c.Conn.Write(p)
}
