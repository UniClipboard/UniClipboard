package wire

import (
	"io"
	"time"
)

// BodyObservation measures successful payload writes only. It is owned by one
// SendStreamObserved call; headers and buffering overhead are excluded.
type BodyObservation struct {
	Bytes       int64
	First, Last time.Time
}

func (o *BodyObservation) Duration() time.Duration {
	if o == nil || o.First.IsZero() {
		return 0
	}
	return o.Last.Sub(o.First)
}

type payloadWriter struct {
	w io.Writer
	o *BodyObservation
}

func (w payloadWriter) Write(p []byte) (int, error) {
	start := time.Now()
	n, err := w.w.Write(p)
	if w.o.First.IsZero() {
		w.o.First = start
	}
	w.o.Last = time.Now()
	w.o.Bytes += int64(n)
	return n, err
}
