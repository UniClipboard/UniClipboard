package e2e

import (
	"bytes"
	"image"
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"testing"

	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/testboard"
	collectmetrics "go.opentelemetry.io/proto/otlp/collector/metrics/v1"
	metricspb "go.opentelemetry.io/proto/otlp/metrics/v1"
	"google.golang.org/protobuf/proto"
)

func TestSyncObservabilityPayloads(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	sender, first, second := newObservabilityRun(t), newObservabilityRun(t), newObservabilityRun(t)
	for i, h := range []*observabilityRun{sender, first, second} {
		h.artifactName = []string{"sync-payload-sender", "sync-payload-first", "sync-payload-second"}[i]
		h.start(t, h.server.URL)
		h.cli(t, "config", "set", "telemetry.enabled", "true")
	}
	pairSyncServices(t, sender, first)
	pairSyncServices(t, sender, second)

	var imageData bytes.Buffer
	if err := png.Encode(&imageData, image.NewRGBA(image.Rect(0, 0, 2, 2))); err != nil {
		t.Fatal(err)
	}
	text, html := []byte("synthetic rich text"), []byte("<b>synthetic rich text</b>")
	file := bytes.Repeat([]byte{0x5a}, 2048)
	bounds := []float64{0, 1024, 65536, 1048576, 16777216, 67108864, 1073741824, 4294967296}
	var totalBytes int64
	for _, fixture := range []struct {
		name, kind string
		parts      []clip.Part
		file       []byte
	}{
		{name: "multipart", kind: "text", parts: []clip.Part{{Format: "text/plain", Data: text}, {Format: "text/html", Data: html}}},
		{name: "image", kind: "image", parts: []clip.Part{{Format: "image/png", Data: imageData.Bytes()}}},
		{name: "file", kind: "files", file: file},
		{name: "empty-file", kind: "files", file: []byte{}},
	} {
		sender.run(t, fixture.name, func(t *testing.T) {
			size := len(fixture.file)
			snapshot := clip.Snapshot{Parts: fixture.parts}
			for _, part := range fixture.parts {
				size += len(part.Data)
			}
			if fixture.file != nil {
				path := filepath.Join(sender.dir, fixture.name+".bin")
				if err := os.WriteFile(path, fixture.file, 0600); err != nil {
					t.Fatal(err)
				}
				snapshot.Files = []clip.File{{Path: path}}
			}
			totalBytes += int64(size)
			type check struct {
				h         *observabilityRun
				direction string
				fanout    uint64
			}
			checks := []check{{sender, "outgoing", 2}, {first, "incoming", 1}, {second, "incoming", 1}}
			previous := make([]*metricspb.HistogramDataPoint, len(checks))
			for i, check := range checks {
				previous[i] = check.h.payloadHistogram(check.direction, fixture.kind)
			}
			if err := testboard.Dir(filepath.Join(sender.home, "board")).Write(snapshot); err != nil {
				t.Fatal(err)
			}
			for i, check := range checks {
				if check.h != sender {
					check.h.eventually(t, func() bool {
						got, err := testboard.Dir(filepath.Join(check.h.home, "board")).Read()
						if err != nil {
							return false
						}
						if fixture.file == nil {
							return slices.EqualFunc(got.Parts, fixture.parts, func(a, b clip.Part) bool {
								return a.Format == b.Format && bytes.Equal(a.Data, b.Data)
							})
						}
						if len(got.Files) != 1 || got.Files[0].Name != fixture.name+".bin" {
							return false
						}
						data, err := os.ReadFile(got.Files[0].Path)
						return err == nil && bytes.Equal(data, fixture.file)
					})
				}
				var actual *metricspb.HistogramDataPoint
				before := previous[i]
				check.h.eventually(t, func() bool {
					actual = check.h.payloadHistogram(check.direction, fixture.kind)
					return actual.GetCount() >= before.GetCount()+check.fanout
				})
				bucket := len(bounds)
				for i, upper := range bounds {
					if float64(size) <= upper {
						bucket = i
						break
					}
				}
				wantBuckets := make([]uint64, len(bounds)+1)
				copy(wantBuckets, before.GetBucketCounts())
				wantBuckets[bucket] += check.fanout
				wantCount := before.GetCount() + check.fanout
				wantSum := before.GetSum() + float64(size)*float64(check.fanout)
				wantBytes := float64(totalBytes) * float64(check.fanout)
				labels := map[string]string{"direction": check.direction, "result": "succeeded"}
				check.h.eventually(t, func() bool {
					return check.h.syncMetric("unic.sync.transfer.bytes", labels) >= wantBytes
				})
				actualBytes := check.h.syncMetric("unic.sync.transfer.bytes", labels)
				check.h.assertions = append(check.h.assertions, map[string]any{
					"scenario": fixture.name, "direction": check.direction, "fixture_bytes": size,
					"expected_count": wantCount, "actual_count": actual.GetCount(), "expected_sum": wantSum, "actual_sum": actual.GetSum(),
					"expected_buckets": wantBuckets, "actual_buckets": actual.GetBucketCounts(), "bounds": actual.GetExplicitBounds(),
					"expected_successful_bytes": wantBytes, "actual_successful_bytes": actualBytes,
				})
				if !slices.Equal(actual.GetExplicitBounds(), bounds) || !slices.Equal(actual.GetBucketCounts(), wantBuckets) ||
					actual.GetCount() != wantCount || actual.GetSum() != wantSum || actualBytes != wantBytes {
					t.Errorf("%s payload metric differs from expected count, sum, buckets or successful bytes", check.h.artifactName)
				}
			}
		})
	}
	for _, h := range []*observabilityRun{sender, first, second} {
		h.assertSyncPrivacy(t)
		h.assertOTLP(t)
	}
}

func (h *observabilityRun) payloadHistogram(direction, kind string) *metricspb.HistogramDataPoint {
	h.mu.Lock()
	defer h.mu.Unlock()
	for i := len(h.requests) - 1; i >= 0; i-- {
		r := h.requests[i]
		if r.Path != "/v1/metrics" {
			continue
		}
		var req collectmetrics.ExportMetricsServiceRequest
		if proto.Unmarshal(r.Body, &req) != nil {
			continue
		}
		for _, resource := range req.ResourceMetrics {
			for _, scope := range resource.ScopeMetrics {
				for _, metric := range scope.Metrics {
					if metric.Name != "unic.sync.transfer.size" {
						continue
					}
					for _, point := range metric.GetHistogram().GetDataPoints() {
						attrs := map[string]string{}
						for _, attr := range point.Attributes {
							attrs[attr.Key] = attr.Value.GetStringValue()
						}
						if len(attrs) == 2 && attrs["direction"] == direction && attrs["content_kind"] == kind {
							return point
						}
					}
				}
			}
		}
	}
	return nil
}
