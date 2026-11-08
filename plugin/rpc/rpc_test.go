package rpc

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"os"
	"strings"
	"sync/atomic"
	"testing"
	"testing/iotest"
	"time"
)

type vectorCase struct {
	Schema  string          `json:"schema"`
	Payload json.RawMessage `json:"payload"`
	Reason  string          `json:"reason"`
}

type vectors struct {
	Valid    []vectorCase `json:"valid_payloads"`
	Invalid  []vectorCase `json:"invalid_payloads"`
	Semantic []vectorCase `json:"semantic_rejections"`
	Golden   []struct {
		Header struct {
			Type uint8 `json:"type"`
		} `json:"header"`
		FrameHex string `json:"frame_hex"`
	} `json:"golden_frames"`
	BadHeaders []struct {
		FrameHex string `json:"frame_hex"`
		Reason   string `json:"reason"`
	} `json:"invalid_headers"`
}

func loadVectors(t *testing.T) vectors {
	t.Helper()
	b, err := os.ReadFile("../../docs/design/protocol/plugin-rpc-v1.vectors.json")
	if err != nil {
		t.Fatal(err)
	}
	var v vectors
	if err := json.Unmarshal(b, &v); err != nil {
		t.Fatal(err)
	}
	return v
}

func firstErr(errs ...error) error {
	for _, e := range errs {
		if e != nil {
			return e
		}
	}
	return nil
}

// decoders for the schemas this package implements; sync_* belong to P5.
var decoders = map[string]func(raw []byte) error{
	"hello":             func(b []byte) error { return Decode(b, new(Hello)) },
	"hello_ack":         func(b []byte) error { return Decode(b, new(HelloAck)) },
	"request":           func(b []byte) error { return Decode(b, new(Request)) },
	"response":          func(b []byte) error { r := new(Response); return firstErr(Decode(b, r), r.validate()) },
	"init":              func(b []byte) error { return Decode(b, new(Init)) },
	"empty":             func(b []byte) error { return Decode(b, new(Empty)) },
	"notify":            func(b []byte) error { return Decode(b, new(Notify)) },
	"stream":            func(b []byte) error { return Decode(b, new(StreamDesc)) },
	"window":            func(b []byte) error { w := new(Window); return firstErr(Decode(b, w), w.validate()) },
	"end":               func(b []byte) error { return Decode(b, new(End)) },
	"process":           func(b []byte) error { p := new(Process); return firstErr(Decode(b, p), p.Validate()) },
	"patch":             func(b []byte) error { p := new(Patch); return firstErr(Decode(b, p), p.Validate()) },
	"action_run":        func(b []byte) error { r := new(ActionRun); return firstErr(Decode(b, r), r.Validate()) },
	"action_result":     func(b []byte) error { return Decode(b, new(ActionResult)) },
	"spec":              func(b []byte) error { return Decode(b, new(Spec)) },
	"ext_submit":        func(b []byte) error { r := new(Submit); return firstErr(Decode(b, r), r.Validate()) },
	"ext_submit_result": func(b []byte) error { return Decode(b, new(SubmitResult)) },
	"ext_leave_channel": func(b []byte) error { r := new(LeaveChannel); return firstErr(Decode(b, r), r.Validate()) },
	"ext_register":      func(b []byte) error { r := new(Register); return firstErr(Decode(b, r), r.Validate()) },
	"ext_report":        func(b []byte) error { r := new(Report); return firstErr(Decode(b, r), r.Validate()) },
	"ext_receive":       func(b []byte) error { r := new(Receive); return firstErr(Decode(b, r), r.Validate()) },
}

func TestVectorsValid(t *testing.T) {
	n := 0
	for i, v := range loadVectors(t).Valid {
		dec, ok := decoders[v.Schema]
		if !ok {
			continue
		}
		n++
		if err := dec(v.Payload); err != nil {
			t.Errorf("valid[%d] %s: %v", i, v.Schema, err)
		}
	}
	if n == 0 {
		t.Fatal("no vectors exercised")
	}
}

func TestVectorsInvalid(t *testing.T) {
	vs := loadVectors(t)
	for i, v := range vs.Invalid {
		if dec, ok := decoders[v.Schema]; ok && dec(v.Payload) == nil {
			t.Errorf("invalid[%d] %s accepted (%s)", i, v.Schema, v.Reason)
		}
	}
	for i, v := range vs.Semantic {
		if dec, ok := decoders[v.Schema]; ok && dec(v.Payload) == nil {
			t.Errorf("semantic[%d] %s accepted (%s)", i, v.Schema, v.Reason)
		}
	}
}

func TestGoldenFrameAnyChunking(t *testing.T) {
	for _, g := range loadVectors(t).Golden {
		want, _ := hex.DecodeString(g.FrameHex)
		f := Frame{Type: g.Header.Type, RequestID: 1, StreamID: 1, Payload: []byte("abc")}
		if got := AppendFrame(nil, f); !bytes.Equal(got, want) {
			t.Fatalf("encoding differs:\n%x\n%x", got, want)
		}
		for _, r := range []io.Reader{bytes.NewReader(want), iotest.OneByteReader(bytes.NewReader(want)), iotest.HalfReader(bytes.NewReader(want))} {
			got, err := ReadFrame(r, DefaultLimits)
			if err != nil || got.Type != f.Type || string(got.Payload) != "abc" || got.RequestID != 1 || got.StreamID != 1 {
				t.Fatalf("read: %+v %v", got, err)
			}
		}
		r := bytes.NewReader(append(bytes.Clone(want), want...))
		for range 2 {
			if _, err := ReadFrame(r, DefaultLimits); err != nil {
				t.Fatal(err)
			}
		}
	}
}

func TestInvalidHeaders(t *testing.T) {
	for _, h := range loadVectors(t).BadHeaders {
		b, _ := hex.DecodeString(h.FrameHex)
		if _, err := ReadFrame(bytes.NewReader(b), DefaultLimits); !errors.Is(err, ErrProtocol) {
			t.Errorf("%s: %v", h.Reason, err)
		}
	}
	// an oversized length is rejected from the header alone, before payload bytes exist.
	b := AppendFrame(nil, Frame{Type: TypeRequest, RequestID: 1, Payload: []byte("{}")})
	b[24], b[25], b[26], b[27] = 0x7f, 0xff, 0xff, 0xff
	if _, err := ReadFrame(bytes.NewReader(b[:HeaderBytes]), DefaultLimits); !errors.Is(err, ErrProtocol) {
		t.Fatalf("got %v", err)
	}
	for _, bad := range []func([]byte){
		func(b []byte) { b[0] = 'X' },
		func(b []byte) { b[5] = 2 },
		func(b []byte) { b[6] = 99 },
		func(b []byte) { b[31] = 1 },
	} {
		c := bytes.Clone(AppendFrame(nil, Frame{Type: TypeRequest, RequestID: 1, Payload: []byte("{}")}))
		bad(c)
		if _, err := ReadFrame(bytes.NewReader(c), DefaultLimits); !errors.Is(err, ErrProtocol) {
			t.Fatalf("accepted %x", c[:HeaderBytes])
		}
	}
}

func TestStrictJSON(t *testing.T) {
	cases := map[string]string{
		"duplicate key":  `{"credit_bytes":"1","credit_bytes":"2"}`,
		"trailing":       `{"credit_bytes":"1"} {}`,
		"unknown field":  `{"credit_bytes":"1","x":1}`,
		"number not str": `{"credit_bytes":1}`,
		"leading zero":   `{"credit_bytes":"01"}`,
		"overflow":       `{"credit_bytes":"18446744073709551616"}`,
		"not object":     `[1]`,
		"invalid utf8":   "{\"credit_bytes\":\"1\"}\xff",
		"deep":           `{"a":` + strings.Repeat(`[`, 40) + strings.Repeat(`]`, 40) + `}`,
		"nested dup":     `{"credit_bytes":"1","z":{"a":1,"a":2}}`,
		"infinity":       `{"credit_bytes":Infinity}`,
	}
	for name, in := range cases {
		if err := Decode([]byte(in), new(Window)); err == nil {
			t.Errorf("%s accepted", name)
		}
	}
	var w Window
	if err := Decode([]byte(`{"credit_bytes":"18446744073709551615"}`), &w); err != nil || w.CreditBytes != 1<<64-1 {
		t.Fatalf("max u64: %v %v", w, err)
	}
}

// pair builds a connected engine/plugin Conn pair.
func pair(t *testing.T, lim Limits, engine, plug Handler, stuck func()) (*Conn, *Conn) {
	t.Helper()
	er, pw := io.Pipe()
	pr, ew := io.Pipe()
	e := NewConn(NewDuplex(er, ew, er, ew), Options{Role: RoleEngine, Limits: lim, Handler: engine, OnStuck: stuck, CancelGrace: 100 * time.Millisecond})
	p := NewConn(NewDuplex(pr, pw, pr, pw), Options{Role: RolePlugin, Limits: lim, Handler: plug})
	t.Cleanup(func() { e.Close(); p.Close() })
	return e, p
}

func TestCallRoundTripAndErrors(t *testing.T) {
	e, _ := pair(t, DefaultLimits, nil, func(ctx context.Context, c *Call) (any, error) {
		switch c.Method {
		case "plugin.start":
			return nil, nil
		case "plugin.process":
			var p Process
			if err := Decode(c.Params, &p); err != nil {
				return nil, err
			}
			return Patch{Generation: p.Generation}, nil
		case "plugin.close":
			return nil, errors.New("secret clipboard text")
		}
		return nil, NewError(CodeUnsupported)
	}, nil)
	ctx := context.Background()
	if _, err := e.Call(ctx, CallOptions{Method: MethodStart, Params: Empty{}}, nil); err != nil {
		t.Fatal(err)
	}
	var patch Patch
	proc := Process{Event: "clipboard.captured", Generation: 9007199254740993, Item: Item{ID: "i", Type: "text", Origin: Origin{Kind: "explicit"}}}
	if _, err := e.Call(ctx, CallOptions{Method: MethodProcess, Params: proc}, &patch); err != nil || patch.Generation != 9007199254740993 {
		t.Fatalf("%+v %v", patch, err)
	}
	_, err := e.Call(ctx, CallOptions{Method: MethodClose, Params: Empty{}}, nil)
	var re *Error
	if !errors.As(err, &re) || re.Code != CodeInternal || strings.Contains(err.Error(), "secret") {
		t.Fatalf("handler text must not cross the wire: %v", err)
	}
	if _, err := e.Call(ctx, CallOptions{Method: "no.such", Params: Empty{}}, nil); !errors.As(err, &re) || re.Code != CodeUnsupported {
		t.Fatalf("got %v", err)
	}
}

func TestCancelPropagatesAndLateResultIsDropped(t *testing.T) {
	started := make(chan struct{})
	finished := make(chan error, 1)
	e, _ := pair(t, DefaultLimits, nil, func(ctx context.Context, c *Call) (any, error) {
		close(started)
		<-ctx.Done()
		finished <- ctx.Err()
		return Empty{}, nil // tries to succeed after cancellation
	}, nil)
	ctx, cancel := context.WithCancel(context.Background())
	errc := make(chan error, 1)
	go func() { _, err := e.Call(ctx, CallOptions{Method: MethodStart, Params: Empty{}}, nil); errc <- err }()
	<-started
	cancel()
	var re *Error
	if err := <-errc; !errors.As(err, &re) || re.Code != CodeCancelled {
		t.Fatalf("got %v", err)
	}
	if err := <-finished; !errors.Is(err, context.Canceled) {
		t.Fatalf("handler context: %v", err)
	}
	time.Sleep(50 * time.Millisecond)
	if e.Err() != nil {
		t.Fatalf("session died: %v", e.Err())
	}
}

func TestTimeoutAndStuckHook(t *testing.T) {
	var stuck atomic.Int32
	block := make(chan struct{})
	defer close(block)
	// a handler that ignores its context never acknowledges the cancel.
	e, _ := pair(t, DefaultLimits, nil, func(ctx context.Context, c *Call) (any, error) { <-block; return nil, nil }, func() { stuck.Add(1) })
	_, err := e.Call(context.Background(), CallOptions{Method: MethodStart, Params: Empty{}, Timeout: 30 * time.Millisecond}, nil)
	var re *Error
	if !errors.As(err, &re) || re.Code != CodeTimeout {
		t.Fatalf("got %v", err)
	}
	deadline := time.Now().Add(2 * time.Second)
	for stuck.Load() == 0 && time.Now().Before(deadline) {
		time.Sleep(10 * time.Millisecond)
	}
	if stuck.Load() == 0 {
		t.Fatal("OnStuck was not invoked")
	}
}

func TestReverseCallNoDeadlock(t *testing.T) {
	var p *Conn
	e, p := pair(t, DefaultLimits, func(ctx context.Context, c *Call) (any, error) {
		if c.Method == MethodNotify {
			return nil, nil
		}
		return nil, NewError(CodeUnsupported)
	}, func(ctx context.Context, c *Call) (any, error) {
		_, err := p.Call(ctx, CallOptions{Method: MethodNotify, Parent: c.ID, Params: Notify{Message: "hi"}}, nil)
		return nil, err
	}, nil)
	if _, err := e.Call(context.Background(), CallOptions{Method: MethodStart, Params: Empty{}}, nil); err != nil {
		t.Fatal(err)
	}
}

func TestStreamFlowControl(t *testing.T) {
	const size = 3<<20 + 123 // larger than one window
	payload := bytes.Repeat([]byte("0123456789abcdef"), size/16+1)[:size]
	got := make(chan []byte, 1)
	e, _ := pair(t, DefaultLimits, nil, func(ctx context.Context, c *Call) (any, error) {
		var p Process
		if err := Decode(c.Params, &p); err != nil {
			return nil, err
		}
		rs, err := c.Conn.Expect(ctx, c.ID, *p.Item.Text, 64<<20)
		if err != nil {
			return nil, err
		}
		b, err := io.ReadAll(rs)
		got <- b
		return Patch{Generation: p.Generation}, err
	}, nil)
	var patch Patch
	_, err := e.Call(context.Background(), CallOptions{Method: MethodProcess, Build: func(id uint64) (any, error) {
		s, d, err := e.NewSend(context.Background(), id, PurposeProcessText, size)
		if err != nil {
			return nil, err
		}
		go func() { _, _ = s.Write(payload); _ = s.Close() }()
		return Process{Event: "clipboard.captured", Generation: 1, Item: Item{ID: "i", Type: "text", Text: &d, Origin: Origin{Kind: "explicit"}}}, nil
	}}, &patch)
	if err != nil {
		t.Fatal(err)
	}
	if b := <-got; !bytes.Equal(b, payload) {
		t.Fatalf("stream corrupted: %d bytes", len(b))
	}
}

func TestTruncatedStreamNeverLooksComplete(t *testing.T) {
	e, _ := pair(t, DefaultLimits, nil, func(ctx context.Context, c *Call) (any, error) {
		var p Process
		if err := Decode(c.Params, &p); err != nil {
			return nil, err
		}
		rs, err := c.Conn.Expect(ctx, c.ID, *p.Item.Text, 1<<20)
		if err != nil {
			return nil, err
		}
		_, err = io.ReadAll(rs)
		return Patch{Generation: p.Generation}, err
	}, nil)
	_, err := e.Call(context.Background(), CallOptions{Method: MethodProcess, Timeout: 300 * time.Millisecond, Build: func(id uint64) (any, error) {
		s, d, _ := e.NewSend(context.Background(), id, PurposeProcessText, 10)
		go func() {
			_, _ = s.Write([]byte("short"))
			if s.Close() == nil {
				t.Error("truncated stream closed successfully")
			}
		}()
		return Process{Event: "clipboard.captured", Generation: 1, Item: Item{ID: "i", Type: "text", Text: &d, Origin: Origin{Kind: "explicit"}}}, nil
	}}, nil)
	if err == nil {
		t.Fatal("call succeeded on a truncated stream")
	}
}

func TestProtocolViolationsEndSession(t *testing.T) {
	send := func(f Frame) *Conn {
		er, pw := io.Pipe()
		_, ew := io.Pipe()
		e := NewConn(NewDuplex(er, ew, er, ew), Options{Role: RoleEngine, Limits: DefaultLimits})
		go func() { _ = WriteFrame(pw, f) }()
		select {
		case <-e.Done():
		case <-time.After(2 * time.Second):
			t.Fatal("session did not end")
		}
		return e
	}
	for name, f := range map[string]Frame{
		"response to nothing":   {Type: TypeResponse, RequestID: 99, Payload: []byte(`{"ok":true,"result":{}}`)},
		"data without stream":   {Type: TypeData, RequestID: 1, StreamID: 7, Payload: []byte("x")},
		"wrong-side request id": {Type: TypeRequest, RequestID: 1, Payload: []byte(`{"method":"x","timeout_ms":"5","parent_request_id":"0","params":{}}`)},
		"hello after start":     {Type: TypeHello, Payload: []byte(`{}`)},
	} {
		if err := send(f).Err(); err == nil {
			t.Errorf("%s: no error", name)
		}
	}
}

func TestHandshake(t *testing.T) {
	spec := Spec{ID: "example.tagger", Version: "1.0.0", APIVersion: 1, Capabilities: []string{"processor"}}
	run := func(expected Spec) (Limits, error, error) {
		er, pw := io.Pipe()
		pr, ew := io.Pipe()
		perr := make(chan error, 1)
		go func() {
			_, err := PluginHandshake(NewDuplex(pr, pw), spec, Limits{ControlBytes: 8192, DataBytes: 4096, StreamWindowBytes: 1 << 17, MaxStreams: 8, MaxRequests: 16})
			perr <- err
		}()
		lim, err := HostHandshake(context.Background(), NewDuplex(er, ew), expected, DefaultLimits, time.Second)
		er.Close()
		pr.Close()
		return lim, err, <-perr
	}
	lim, herr, perr := run(spec)
	if herr != nil || perr != nil || lim.DataBytes != 4096 || lim.MaxRequests != 16 {
		t.Fatalf("%+v %v %v", lim, herr, perr)
	}
	other := spec
	other.ID = "example.other"
	if _, herr, _ := run(other); herr == nil {
		t.Fatal("identity mismatch accepted")
	}
	other = spec
	other.Capabilities = []string{"processor", "action"}
	if _, herr, _ := run(other); herr == nil {
		t.Fatal("capability mismatch accepted")
	}
}
