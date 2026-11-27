//go:build darwin

package sampler

import (
	"context"
	"os"
	"testing"
)

func TestDarwinSelfSampleAndAttributionParsing(t *testing.T) {
	pid := os.Getpid()
	ps, err := SampleProcess(pid)
	if err != nil {
		t.Fatalf("SampleProcess(%d): %v", pid, err)
	}
	if ps.PhysFootprintBytes == 0 || ps.PrimaryMemoryBytes == 0 {
		t.Fatalf("expected phys_footprint > 0, got %+v", ps)
	}
	if ps.CPUTotalNS == 0 {
		t.Fatalf("expected cpu_total_ns > 0, got %+v", ps)
	}
	if ps.FDs <= 0 {
		t.Fatalf("expected fds > 0, got %+v", ps)
	}
	if ps.Threads <= 0 {
		t.Fatalf("expected threads > 0, got %+v", ps)
	}

	// Test footprint -j JSON parser with realistic macOS schema.
	sampleJSON := []byte(`{
		"unit":"byte",
		"total footprint": 10485760,
		"page size": 16384,
		"processes":[{
			"name":"unic-desktop",
			"pid":12345,
			"translated":false,
			"page size":16384,
			"footprint":10485760,
			"categories":{
				"IOSurface":{"dirty":4194304,"swapped":0,"clean":0,"reclaimable":0,"wired":0,"regions":2},
				"IOAccelerator":{"dirty":2097152,"swapped":0,"clean":0,"reclaimable":0,"wired":0,"regions":1},
				"Malloc Small":{"dirty":1048576,"swapped":0,"clean":0,"reclaimable":0,"wired":0,"regions":3},
				"VM_ALLOCATE":{"dirty":524288,"swapped":0,"clean":0,"reclaimable":0,"wired":0,"regions":2},
				"Owned physical footprint (unmapped)":{"dirty":262144,"swapped":0,"clean":0,"reclaimable":0,"wired":0,"regions":1}
			},
			"auxiliary":{"phys_footprint":10485760,"phys_footprint_peak":12582912}
		}]
	}`)
	rep, err := ParseFootprintJSON(sampleJSON)
	if err != nil {
		t.Fatalf("ParseFootprintJSON: %v", err)
	}
	if len(rep.Processes) != 1 || rep.Processes[0].Auxiliary.PhysFootprintPeak != 12582912 {
		t.Fatalf("unexpected footprint report: %+v", rep)
	}

	topOut := "Processes: 500 total\nPID    #PORTS\n12345  42+\n"
	ports := ParseTopPorts(topOut)
	if ports[12345] != 42 {
		t.Fatalf("ParseTopPorts = %+v, want pid 12345 -> 42", ports)
	}

	// Also smoke-test single-shot CaptureAttribution on self.
	attr, err := CaptureAttribution(context.Background(), "self-test", map[string]int{"self": pid})
	if err != nil {
		t.Fatalf("CaptureAttribution: %v", err)
	}
	pa, ok := attr.Processes[pid]
	if !ok || pa.PhysFootprintBytes == 0 {
		t.Fatalf("expected attribution for pid %d, got %+v (warnings=%v)", pid, attr.Processes, attr.Warnings)
	}
}
