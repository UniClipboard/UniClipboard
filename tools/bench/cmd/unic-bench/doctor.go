package main

import (
	"context"
	"crypto/sha256"
	"debug/macho"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"

	"github.com/uniclipboard/unic/tools/bench/internal/sampler"
)

type doctorCheck struct {
	Name   string `json:"name"`
	Status string `json:"status"` // ok, warn, fail
	Detail string `json:"detail"`
}

type doctorBinary struct {
	Name       string `json:"name"`
	Path       string `json:"path"`
	SHA256     string `json:"sha256"`
	HostArch   string `json:"host_arch"`
	BinaryArch string `json:"binary_arch"`
	Emulated   bool   `json:"emulated"`
}

type doctorReport struct {
	Status       string                `json:"status"` // ok, warn, fail
	Host         sampler.HostInfo      `json:"host"`
	SelfSample   sampler.ProcessSample `json:"self_sample"`
	ClipboardSeq *uint64               `json:"clipboard_sequence,omitempty"`
	Checks       []doctorCheck         `json:"checks"`
	Binaries     []doctorBinary        `json:"binaries,omitempty"`
}

func cmdDoctor(ctx context.Context, args []string, stdout, stderr io.Writer) error {
	fs := flag.NewFlagSet("doctor", flag.ContinueOnError)
	fs.SetOutput(stderr)
	unicBin := fs.String("unic", "", "optional path to unic binary")
	unicdBin := fs.String("unicd", "", "optional path to unicd binary")
	desktopBin := fs.String("desktop", "", "optional path to unic-desktop binary")
	asJSON := fs.Bool("json", false, "output JSON")
	if err := fs.Parse(args); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return nil
		}
		return &exitError{code: exitUsage, err: err}
	}
	if fs.NArg() != 0 {
		return &exitError{code: exitUsage, err: fmt.Errorf("unexpected positional arguments: %v", fs.Args())}
	}

	rep := runDoctorChecks(ctx, map[string]string{
		"unic":         *unicBin,
		"unicd":        *unicdBin,
		"unic-desktop": *desktopBin,
	})

	if *asJSON {
		enc := json.NewEncoder(stdout)
		enc.SetIndent("", "  ")
		if err := enc.Encode(rep); err != nil {
			return err
		}
	} else {
		printDoctorText(stdout, rep)
	}

	if rep.Status == "fail" {
		return &exitError{code: exitFailed, err: errors.New("doctor detected one or more failed checks")}
	}
	return nil
}

func runDoctorChecks(ctx context.Context, binFlags map[string]string) doctorReport {
	host := sampler.CollectHostInfo(ctx)
	rep := doctorReport{
		Status: "ok",
		Host:   host,
	}

	add := func(name, status, detail string) {
		rep.Checks = append(rep.Checks, doctorCheck{Name: name, Status: status, Detail: detail})
		if status == "fail" {
			rep.Status = "fail"
		} else if status == "warn" && rep.Status == "ok" {
			rep.Status = "warn"
		}
	}

	// 1. Self-sampling check.
	self, err := sampler.SampleProcess(os.Getpid())
	if err != nil {
		add("self_sample", "fail", err.Error())
	} else {
		rep.SelfSample = self
		if self.PrimaryMemoryBytes == 0 || self.Threads <= 0 {
			add("self_sample", "fail", fmt.Sprintf("incomplete self sample: mem=%d threads=%d", self.PrimaryMemoryBytes, self.Threads))
		} else {
			add("self_sample", "ok", fmt.Sprintf("pid=%d primary_mem=%d threads=%d cpu_ns=%d", self.PID, self.PrimaryMemoryBytes, self.Threads, self.CPUTotalNS))
		}
	}

	// 2. Clipboard sequence number probe (read-only).
	if seq, err := sampler.ClipboardSequence(); err != nil {
		add("clipboard_sequence", "warn", err.Error())
	} else {
		rep.ClipboardSeq = &seq
		add("clipboard_sequence", "ok", fmt.Sprintf("sequence=%d", seq))
	}

	// 3. Platform-specific tools / permissions.
	switch runtime.GOOS {
	case "darwin":
		for _, tool := range []string{"/usr/bin/footprint", "/usr/bin/top", "/usr/bin/vmmap", "/bin/ps"} {
			if st, err := os.Stat(tool); err == nil && !st.IsDir() {
				add("tool:"+filepath.Base(tool), "ok", tool)
			} else {
				add("tool:"+filepath.Base(tool), "fail", fmt.Sprintf("missing %s", tool))
			}
		}
		if _, err := exec.LookPath("prlctl"); err == nil {
			add("parallels_prlctl", "ok", "prlctl available")
		} else {
			add("parallels_prlctl", "warn", "prlctl not found (only needed for Windows VM orchestration)")
		}
		if attr, err := sampler.CaptureAttribution(ctx, "doctor", map[string]int{"self": os.Getpid()}); err != nil {
			add("attribution_self", "warn", err.Error())
		} else if len(attr.Warnings) > 0 {
			add("attribution_self", "warn", strings.Join(attr.Warnings, "; "))
		} else {
			add("attribution_self", "ok", fmt.Sprintf("footprint categories=%d", len(attr.Processes[os.Getpid()].Categories)))
		}
	case "windows":
		if attr, err := sampler.CaptureAttribution(ctx, "doctor", map[string]int{"self": os.Getpid()}); err != nil {
			add("virtual_query_ex", "warn", err.Error())
		} else if len(attr.Warnings) > 0 {
			add("virtual_query_ex", "warn", strings.Join(attr.Warnings, "; "))
		} else {
			pa := attr.Processes[os.Getpid()]
			add("virtual_query_ex", "ok", fmt.Sprintf("private_commit=%d image=%d", pa.PrivateCommittedBytes, pa.ImageCommittedBytes))
		}
	default:
		add("platform", "warn", "native sampling is only supported on darwin and windows")
	}

	// 4. Optional binary architecture inspection.
	for _, name := range []string{"unic", "unicd", "unic-desktop"} {
		path := binFlags[name]
		if path == "" {
			continue
		}
		bi, err := inspectDoctorBinary(name, path, host.HostArch)
		if err != nil {
			add("binary:"+name, "fail", err.Error())
			continue
		}
		rep.Binaries = append(rep.Binaries, bi)
		status := "ok"
		if bi.Emulated {
			status = "warn"
		}
		add("binary:"+name, status, fmt.Sprintf("host=%s binary=%s emulated=%t path=%s", bi.HostArch, bi.BinaryArch, bi.Emulated, bi.Path))
	}

	// 5. Read-only check for pre-existing user unic processes (never terminates them).
	if running := detectExistingUnicProcesses(ctx); len(running) > 0 {
		add("existing_unic_processes", "warn", fmt.Sprintf("detected active unic processes (%s); keep benchmarks in an isolated UNIC_HOME", strings.Join(running, ", ")))
	} else {
		add("existing_unic_processes", "ok", "no active unicd/unic-desktop processes detected")
	}

	return rep
}

func inspectDoctorBinary(name, path, hostArch string) (doctorBinary, error) {
	abs, err := filepath.Abs(path)
	if err != nil {
		return doctorBinary{}, err
	}
	data, err := os.ReadFile(abs)
	if err != nil {
		return doctorBinary{}, err
	}
	sum := sha256.Sum256(data)
	out := doctorBinary{
		Name:     name,
		Path:     abs,
		SHA256:   hex.EncodeToString(sum[:]),
		HostArch: hostArch,
	}

	if peInfo, err := sampler.InspectPE(abs, hostArch); err == nil {
		out.BinaryArch = peInfo.BinaryArch
		out.Emulated = peInfo.Emulated
		return out, nil
	}
	if mf, err := macho.Open(abs); err == nil {
		defer func() { _ = mf.Close() }()
		switch mf.Cpu {
		case macho.CpuArm64:
			out.BinaryArch = "arm64"
		case macho.CpuAmd64:
			out.BinaryArch = "amd64"
		default:
			out.BinaryArch = mf.Cpu.String()
		}
		out.Emulated = out.HostArch != "" && out.BinaryArch != "" && out.HostArch != out.BinaryArch
		return out, nil
	}
	if fmf, err := macho.OpenFat(abs); err == nil {
		defer func() { _ = fmf.Close() }()
		var arches []string
		for _, arch := range fmf.Arches {
			arches = append(arches, arch.Cpu.String())
		}
		out.BinaryArch = "universal(" + strings.Join(arches, ",") + ")"
		return out, nil
	}
	out.BinaryArch = runtime.GOARCH
	return out, nil
}

func detectExistingUnicProcesses(ctx context.Context) []string {
	switch runtime.GOOS {
	case "darwin":
		out, err := exec.CommandContext(ctx, "/bin/ps", "-axo", "pid=,comm=").Output()
		if err != nil {
			return nil
		}
		var hits []string
		for _, line := range strings.Split(string(out), "\n") {
			fields := strings.Fields(line)
			if len(fields) < 2 {
				continue
			}
			base := filepath.Base(fields[1])
			if base == "unicd" || base == "unic-desktop" {
				hits = append(hits, fmt.Sprintf("%s:%s", base, fields[0]))
			}
		}
		return hits
	case "windows":
		out, err := exec.CommandContext(ctx, "tasklist", "/FO", "CSV", "/NH").Output()
		if err != nil {
			return nil
		}
		var hits []string
		for _, line := range strings.Split(string(out), "\n") {
			lower := strings.ToLower(line)
			if strings.Contains(lower, "unicd.exe") || strings.Contains(lower, "unic-desktop.exe") {
				hits = append(hits, strings.TrimSpace(line))
			}
		}
		return hits
	default:
		return nil
	}
}

func printDoctorText(w io.Writer, rep doctorReport) {
	_, _ = fmt.Fprintf(w, "unic-bench doctor: status=%s\n", rep.Status)
	_, _ = fmt.Fprintf(w, "Host: %s/%s os=%s cpus=%d ram=%dB vm=%t cpu=%s\n",
		rep.Host.OS, rep.Host.HostArch, rep.Host.OSVersion, rep.Host.LogicalCPUs,
		rep.Host.PhysicalMemoryBytes, rep.Host.VirtualMachine, rep.Host.CPUModel)
	for _, c := range rep.Checks {
		_, _ = fmt.Fprintf(w, "  [%-4s] %-24s %s\n", strings.ToUpper(c.Status), c.Name, c.Detail)
	}
}
