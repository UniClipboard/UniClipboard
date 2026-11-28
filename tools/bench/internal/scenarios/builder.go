package scenarios

import (
	"context"
	"crypto/sha256"
	"debug/macho"
	"encoding/hex"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"

	"github.com/uniclipboard/unic/tools/bench/internal/report"
	"github.com/uniclipboard/unic/tools/bench/internal/sampler"
)

// FindRepoRoot walks up from wd (or os.Getwd) to locate go.mod.
func FindRepoRoot(start string) (string, error) {
	if start == "" {
		wd, err := os.Getwd()
		if err != nil {
			return "", err
		}
		start = wd
	}
	cur, err := filepath.Abs(start)
	if err != nil {
		return "", err
	}
	for {
		if st, err := os.Stat(filepath.Join(cur, "go.mod")); err == nil && !st.IsDir() {
			return cur, nil
		}
		parent := filepath.Dir(cur)
		if parent == cur {
			return "", fmt.Errorf("could not locate go.mod from %s", start)
		}
		cur = parent
	}
}

// PrepareBinaries ensures required Track A / Track B binaries are available,
// building any missing targets inside outputDir/bin without -race.
// Production binaries live together in bin/prod/ (so sibling resolution finds unicd)
// and Track B binaries live together in bin/e2e-probe/.
func PrepareBinaries(ctx context.Context, opts *Options) (map[string]report.BinaryInfo, error) {
	needProd := opts.Track == TrackProduction || opts.Track == TrackBoth
	needInteractive := opts.Track == TrackInteractive || opts.Track == TrackBoth

	prodDir := filepath.Join(opts.OutputDir, "bin", "prod")
	probeDir := filepath.Join(opts.OutputDir, "bin", "e2e-probe")

	exeExt := ""
	if runtime.GOOS == "windows" {
		exeExt = ".exe"
	}

	buildOne := func(dst string, pkg string, tags []string, isWindowsGUI bool) error {
		if err := os.MkdirAll(filepath.Dir(dst), 0o755); err != nil {
			return err
		}
		ldflags := "-s -w"
		if runtime.GOOS == "windows" && isWindowsGUI {
			ldflags += " -H windowsgui"
		}
		args := []string{"build", "-trimpath", "-buildvcs=false", "-ldflags=" + ldflags}
		if len(tags) > 0 {
			args = append(args, "-tags", strings.Join(tags, ","))
		}
		args = append(args, "-o", dst, pkg)
		cmd := exec.CommandContext(ctx, "go", args...)
		cmd.Dir = opts.RepoRoot
		if out, err := cmd.CombinedOutput(); err != nil {
			return fmt.Errorf("go %s: %w\n%s", strings.Join(args, " "), err, strings.TrimSpace(string(out)))
		}
		return nil
	}

	// Ensure sibling binaries exist in the same directory when any Track A or Track B
	// binary needs to be built so unic-desktop's lifecycle.Sibling() finds the matching unicd.
	if opts.Binaries.Unic == "" {
		dst := filepath.Join(prodDir, "unic"+exeExt)
		if opts.Logf != nil {
			opts.Logf("building production unic -> %s", dst)
		}
		if err := buildOne(dst, "./app/service/cmd/unic", nil, false); err != nil {
			return nil, err
		}
		opts.Binaries.Unic = dst
	}
	if needProd {
		if opts.Binaries.Unicd == "" {
			dst := filepath.Join(prodDir, "unicd"+exeExt)
			if opts.Logf != nil {
				opts.Logf("building production unicd -> %s", dst)
			}
			if err := buildOne(dst, "./app/service/cmd/unicd", nil, false); err != nil {
				return nil, err
			}
			opts.Binaries.Unicd = dst
		}
		if opts.Binaries.Desktop == "" {
			dst := filepath.Join(prodDir, "unic-desktop"+exeExt)
			if opts.Logf != nil {
				opts.Logf("building production unic-desktop -> %s", dst)
			}
			if err := buildOne(dst, "./app/desktop/cmd/unic-desktop", nil, true); err != nil {
				return nil, err
			}
			opts.Binaries.Desktop = dst
		}
		if err := ensureSiblingPair(opts.Binaries.Desktop, opts.Binaries.Unicd, "unicd"+exeExt); err != nil {
			return nil, err
		}
	}

	if needInteractive {
		if opts.Binaries.UnicdProbe == "" {
			dst := filepath.Join(probeDir, "unicd"+exeExt)
			if opts.Logf != nil {
				opts.Logf("building bench_probe unicd -> %s", dst)
			}
			if err := buildOne(dst, "./app/service/cmd/unicd", []string{"bench_probe"}, false); err != nil {
				return nil, err
			}
			opts.Binaries.UnicdProbe = dst
		}
		if opts.Binaries.DesktopE2EProbe == "" {
			dst := filepath.Join(probeDir, "unic-desktop"+exeExt)
			if opts.Logf != nil {
				opts.Logf("building desktop_e2e,bench_probe unic-desktop -> %s", dst)
			}
			if err := buildOne(dst, "./app/desktop/cmd/unic-desktop", []string{"desktop_e2e", "bench_probe"}, true); err != nil {
				return nil, err
			}
			opts.Binaries.DesktopE2EProbe = dst
		}
		if err := ensureSiblingPair(opts.Binaries.DesktopE2EProbe, opts.Binaries.UnicdProbe, "unicd"+exeExt); err != nil {
			return nil, err
		}
	}

	hostArch := sampler.HostArch()
	info := map[string]report.BinaryInfo{}
	record := func(key, path string, tags []string) error {
		if path == "" {
			return nil
		}
		bi, err := collectBinaryInfo(path, hostArch, tags)
		if err != nil {
			return err
		}
		info[key] = bi
		return nil
	}
	if err := record("unic", opts.Binaries.Unic, nil); err != nil {
		return nil, err
	}
	if err := record("unicd", opts.Binaries.Unicd, nil); err != nil {
		return nil, err
	}
	if err := record("unic-desktop", opts.Binaries.Desktop, nil); err != nil {
		return nil, err
	}
	if err := record("unicd-bench-probe", opts.Binaries.UnicdProbe, []string{"bench_probe"}); err != nil {
		return nil, err
	}
	if err := record("unic-desktop-e2e-probe", opts.Binaries.DesktopE2EProbe, []string{"desktop_e2e", "bench_probe"}); err != nil {
		return nil, err
	}
	return info, nil
}

func ensureSiblingPair(desktopBin, daemonBin, daemonName string) error {
	if desktopBin == "" || daemonBin == "" {
		return nil
	}
	want := filepath.Join(filepath.Dir(desktopBin), daemonName)
	absWant, _ := filepath.Abs(want)
	absHave, _ := filepath.Abs(daemonBin)
	if absWant == absHave {
		return nil
	}
	if _, err := os.Stat(want); err == nil {
		return nil
	}
	data, err := os.ReadFile(absHave)
	if err != nil {
		return fmt.Errorf("copy sibling %s: %w", daemonName, err)
	}
	return os.WriteFile(want, data, 0o755)
}

func collectBinaryInfo(path, hostArch string, tags []string) (report.BinaryInfo, error) {
	abs, err := filepath.Abs(path)
	if err != nil {
		return report.BinaryInfo{}, err
	}
	data, err := os.ReadFile(abs)
	if err != nil {
		return report.BinaryInfo{}, err
	}
	sum := sha256.Sum256(data)
	bi := report.BinaryInfo{
		Path:      abs,
		SHA256:    hex.EncodeToString(sum[:]),
		HostArch:  hostArch,
		BuildTags: tags,
	}
	if peInfo, err := sampler.InspectPE(abs, hostArch); err == nil {
		bi.BinaryArch = peInfo.BinaryArch
		bi.Emulated = peInfo.Emulated
		return bi, nil
	}
	if mf, err := macho.Open(abs); err == nil {
		defer func() { _ = mf.Close() }()
		switch mf.Cpu {
		case macho.CpuArm64:
			bi.BinaryArch = "arm64"
		case macho.CpuAmd64:
			bi.BinaryArch = "amd64"
		default:
			bi.BinaryArch = mf.Cpu.String()
		}
		bi.Emulated = hostArch != "" && bi.BinaryArch != "" && hostArch != bi.BinaryArch
		return bi, nil
	}
	bi.BinaryArch = runtime.GOARCH
	return bi, nil
}

func collectGitInfo(repoRoot string) (commit string, dirty bool) {
	if repoRoot == "" {
		return "", false
	}
	if out, err := exec.Command("git", "-C", repoRoot, "rev-parse", "--short=12", "HEAD").Output(); err == nil {
		commit = strings.TrimSpace(string(out))
	}
	if out, err := exec.Command("git", "-C", repoRoot, "status", "--porcelain").Output(); err == nil {
		dirty = strings.TrimSpace(string(out)) != ""
	}
	return commit, dirty
}
