package sampler

import (
	"bytes"
	"debug/pe"
	"encoding/binary"
	"math"
	"testing"
)

func TestTrimWarmupAndSummarizeValues(t *testing.T) {
	samples := []GroupSample{
		{SampleIndex: 0, ElapsedSec: 0, UTCMillis: 1000, TotalPrimaryMemoryBytes: 500 << 20},
		{SampleIndex: 1, ElapsedSec: 5, UTCMillis: 6000, TotalPrimaryMemoryBytes: 400 << 20},
		{SampleIndex: 2, ElapsedSec: 15, UTCMillis: 16000, TotalPrimaryMemoryBytes: 100 << 20},
		{SampleIndex: 3, ElapsedSec: 25, UTCMillis: 26000, TotalPrimaryMemoryBytes: 110 << 20},
		{SampleIndex: 4, ElapsedSec: 35, UTCMillis: 36000, TotalPrimaryMemoryBytes: 120 << 20},
		{SampleIndex: 5, ElapsedSec: 45, UTCMillis: 46000, TotalPrimaryMemoryBytes: 130 << 20},
	}

	steady, trimmed := TrimWarmup(samples, 15.0)
	if trimmed != 2 {
		t.Fatalf("trimmed = %d, want 2", trimmed)
	}
	if len(steady) != 4 {
		t.Fatalf("len(steady) = %d, want 4", len(steady))
	}

	vals := []float64{10, 20, 30, 40, 50, 60, 70, 80, 90, 100}
	stats := SummarizeValues(vals)
	if stats.Count != 10 || stats.Min != 10 || stats.Max != 100 {
		t.Fatalf("unexpected min/max/count: %+v", stats)
	}
	if math.Abs(stats.Median-55) > 1e-6 {
		t.Fatalf("median = %v, want 55", stats.Median)
	}
	if math.Abs(stats.P95-95.5) > 1e-6 {
		t.Fatalf("p95 = %v, want 95.5", stats.P95)
	}
}

func TestLinearRegressionSlope(t *testing.T) {
	xs := []float64{0, 1, 2, 3, 4}
	ys := []float64{100, 102, 104, 106, 108}
	slope, intercept := LinearRegressionSlope(xs, ys)
	if math.Abs(slope-2.0) > 1e-9 || math.Abs(intercept-100.0) > 1e-9 {
		t.Fatalf("got slope=%v intercept=%v, want 2.0 and 100.0", slope, intercept)
	}
}

func TestSummarizePhase(t *testing.T) {
	samples := []GroupSample{
		{
			SampleIndex: 0, ElapsedSec: 0, UTCMillis: 1000, TotalPrimaryMemoryBytes: 300 << 20,
			Roles: map[string]ProcessSample{
				"unicd": {Role: "unicd", PID: 101, PrimaryMemoryBytes: 300 << 20, CPUTotalNS: 100_000_000, IdleWakeups: 10, Threads: 4, FDs: 12},
			},
		},
		{
			SampleIndex: 1, ElapsedSec: 10, UTCMillis: 11000, TotalPrimaryMemoryBytes: 100 << 20,
			TotalCPUTotalNS: 200_000_000, TotalIdleWakeups: 20,
			Roles: map[string]ProcessSample{
				"unicd": {Role: "unicd", PID: 101, PrimaryMemoryBytes: 100 << 20, CPUTotalNS: 200_000_000, IdleWakeups: 20, Threads: 5, FDs: 14},
			},
		},
		{
			SampleIndex: 2, ElapsedSec: 20, UTCMillis: 21000, TotalPrimaryMemoryBytes: 110 << 20,
			TotalCPUTotalNS: 300_000_000, TotalIdleWakeups: 30,
			Roles: map[string]ProcessSample{
				"unicd": {Role: "unicd", PID: 101, PrimaryMemoryBytes: 110 << 20, CPUTotalNS: 300_000_000, IdleWakeups: 30, Threads: 5, FDs: 14},
			},
		},
		{
			SampleIndex: 3, ElapsedSec: 30, UTCMillis: 31000, TotalPrimaryMemoryBytes: 120 << 20,
			TotalCPUTotalNS: 400_000_000, TotalIdleWakeups: 40,
			Roles: map[string]ProcessSample{
				"unicd": {Role: "unicd", PID: 101, PrimaryMemoryBytes: 120 << 20, CPUTotalNS: 400_000_000, IdleWakeups: 40, Threads: 6, FDs: 15},
			},
		},
	}

	sum := SummarizePhase("bg-idle", samples, []string{"unicd"}, 10.0, 10.0)
	if sum.WarmupTrimmedSamples != 1 || sum.SampleCount != 3 {
		t.Fatalf("unexpected warmup/sample count: %+v", sum)
	}
	if sum.EarlyPeakBytes != 300<<20 {
		t.Fatalf("EarlyPeakBytes = %d, want %d", sum.EarlyPeakBytes, uint64(300<<20))
	}
	if sum.SteadyMedianBytes != 110<<20 {
		t.Fatalf("SteadyMedianBytes = %d, want %d", sum.SteadyMedianBytes, uint64(110<<20))
	}
	if math.Abs(sum.CPUSeconds-0.2) > 1e-9 {
		t.Fatalf("CPUSeconds = %v, want 0.2", sum.CPUSeconds)
	}
	if math.Abs(sum.CPUPercentMean-1.0) > 1e-6 {
		t.Fatalf("CPUPercentMean = %v, want 1.0", sum.CPUPercentMean)
	}
	if math.Abs(sum.IdleWakeupsPerSec-1.0) > 1e-6 {
		t.Fatalf("IdleWakeupsPerSec = %v, want 1.0", sum.IdleWakeupsPerSec)
	}
	if math.Abs(sum.LeakSlopeBytesPerSample-float64(10<<20)) > 1.0 {
		t.Fatalf("LeakSlopeBytesPerSample = %v, want %v", sum.LeakSlopeBytesPerSample, float64(10<<20))
	}
}

func makeMinimalPE(machine uint16) []byte {
	// DOS header (>= 96 bytes required by debug/pe), e_lfanew at 0x3c = 128,
	// "PE\0\0" signature, and 20-byte COFF FileHeader.
	buf := make([]byte, 128+4+20)
	buf[0] = 'M'
	buf[1] = 'Z'
	binary.LittleEndian.PutUint32(buf[0x3c:0x40], 128)
	copy(buf[128:132], []byte{'P', 'E', 0, 0})
	binary.LittleEndian.PutUint16(buf[132:134], machine)
	return buf
}

func TestInspectPEReader(t *testing.T) {
	amd64PE := makeMinimalPE(pe.IMAGE_FILE_MACHINE_AMD64)
	arm64PE := makeMinimalPE(pe.IMAGE_FILE_MACHINE_ARM64)

	info1, err := InspectPEReader(bytes.NewReader(amd64PE), "arm64")
	if err != nil {
		t.Fatalf("InspectPEReader amd64 on arm64: %v", err)
	}
	if info1.HostArch != "arm64" || info1.BinaryArch != "amd64" || !info1.Emulated {
		t.Fatalf("unexpected PEInfo for amd64 on arm64: %+v", info1)
	}

	info2, err := InspectPEReader(bytes.NewReader(arm64PE), "arm64")
	if err != nil {
		t.Fatalf("InspectPEReader arm64 on arm64: %v", err)
	}
	if info2.HostArch != "arm64" || info2.BinaryArch != "arm64" || info2.Emulated {
		t.Fatalf("unexpected PEInfo for arm64 on arm64: %+v", info2)
	}
}
