package observability

import (
	"bytes"
	"errors"
	"io"
	"net/http"
	"sync/atomic"

	collectlogs "go.opentelemetry.io/proto/otlp/collector/logs/v1"
	common "go.opentelemetry.io/proto/otlp/common/v1"
	resourcepb "go.opentelemetry.io/proto/otlp/resource/v1"
	"google.golang.org/protobuf/proto"
)

// OTel v1.47 WithResource merges environment attributes. The log SDK exposes
// no Record.SetResource. This transport applies the final allowlist through
// the public OTLP protobuf model, before any byte reaches the network.
type privateTransport struct {
	next     http.RoundTripper
	resource *resourcepb.Resource
	status   atomic.Int64
}

func (t *privateTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	if req.URL.Path != "/v1/logs" && req.URL.Path != "/i/v1/logs" {
		return t.roundTrip(req)
	}
	b, err := io.ReadAll(io.LimitReader(req.Body, 2<<20))
	_ = req.Body.Close()
	if err != nil {
		return nil, errors.New("invalid diagnostic batch")
	}
	var data collectlogs.ExportLogsServiceRequest
	if err := proto.Unmarshal(b, &data); err != nil {
		return nil, errors.New("invalid diagnostic batch")
	}
	for _, r := range data.ResourceLogs {
		r.Resource = proto.Clone(t.resource).(*resourcepb.Resource)
		r.SchemaUrl = ""
		for _, s := range r.ScopeLogs {
			if s.Scope != nil {
				s.Scope.Attributes = nil
				s.Scope.DroppedAttributesCount = 0
			}
			s.SchemaUrl = ""
		}
	}
	b, err = proto.Marshal(&data)
	if err != nil {
		return nil, errors.New("invalid diagnostic batch")
	}
	clone := req.Clone(req.Context())
	clone.Body = io.NopCloser(bytes.NewReader(b))
	clone.ContentLength = int64(len(b))
	clone.GetBody = nil
	return t.roundTrip(clone)
}
func resourceAttribute(k, v string) *common.KeyValue {
	return &common.KeyValue{Key: k, Value: &common.AnyValue{Value: &common.AnyValue_StringValue{StringValue: v}}}
}

func (t *privateTransport) roundTrip(req *http.Request) (*http.Response, error) {
	t.status.Store(0)
	response, err := t.next.RoundTrip(req)
	if response != nil {
		t.status.Store(int64(response.StatusCode))
	}
	return response, err
}
