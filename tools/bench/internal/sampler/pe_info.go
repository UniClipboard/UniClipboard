package sampler

import (
	"debug/pe"
	"fmt"
	"io"
	"os"
	"runtime"
)

// PEInfo records authoritative PE architecture details parsed from the binary
// header rather than relying on WOW64 heuristics.
type PEInfo struct {
	HostArch    string `json:"host_arch"`
	BinaryArch  string `json:"binary_arch"`
	Emulated    bool   `json:"emulated"`
	Machine     uint16 `json:"machine"`
	MachineName string `json:"machine_name"`
}

// InspectPE parses a Windows PE binary file at exePath and reports host_arch,
// binary_arch, and whether the binary runs emulated on hostArch (defaults to HostArch() when empty).
func InspectPE(exePath string, hostArch ...string) (*PEInfo, error) {
	f, err := os.Open(exePath)
	if err != nil {
		return nil, fmt.Errorf("open PE %s: %w", exePath, err)
	}
	defer func() { _ = f.Close() }()
	h := ""
	if len(hostArch) > 0 {
		h = hostArch[0]
	}
	return InspectPEReader(f, h)
}

// InspectPEReader parses a PE image from r and determines binary_arch and emulation status.
func InspectPEReader(r io.ReaderAt, hostArch string) (*PEInfo, error) {
	pf, err := pe.NewFile(r)
	if err != nil {
		return nil, fmt.Errorf("parse PE header: %w", err)
	}
	defer func() { _ = pf.Close() }()

	if hostArch == "" {
		hostArch = HostArch()
		if hostArch == "" {
			hostArch = runtime.GOARCH
		}
	}

	binArch, machineName := peMachineArch(pf.Machine)
	return &PEInfo{
		HostArch:    hostArch,
		BinaryArch:  binArch,
		Emulated:    isEmulatedOnHost(hostArch, binArch),
		Machine:     pf.Machine,
		MachineName: machineName,
	}, nil
}

func peMachineArch(machine uint16) (arch, name string) {
	switch machine {
	case pe.IMAGE_FILE_MACHINE_AMD64:
		return "amd64", "IMAGE_FILE_MACHINE_AMD64"
	case pe.IMAGE_FILE_MACHINE_ARM64:
		return "arm64", "IMAGE_FILE_MACHINE_ARM64"
	case pe.IMAGE_FILE_MACHINE_I386:
		return "386", "IMAGE_FILE_MACHINE_I386"
	case pe.IMAGE_FILE_MACHINE_ARMNT:
		return "arm", "IMAGE_FILE_MACHINE_ARMNT"
	default:
		hexCode := fmt.Sprintf("0x%04x", machine)
		return hexCode, hexCode
	}
}

func isEmulatedOnHost(hostArch, binArch string) bool {
	if hostArch == "" || binArch == "" {
		return false
	}
	return hostArch != binArch
}
