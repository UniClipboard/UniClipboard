package luascripts

import (
	"bytes"
	"context"
	"debug/elf"
	"debug/macho"
	"debug/pe"
	"encoding/json"
	"errors"
	"io"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
)

func (g githubRepo) prepareGo(ctx context.Context, stage, work, bundle string, m Manifest, build bool, pin *installedPlugin) error {
	s := Script{Dir: bundle}
	out := s.Executable()
	if err := os.MkdirAll(filepath.Dir(out), 0700); err != nil {
		return err
	}
	pin.GOOS, pin.GOARCH = runtime.GOOS, runtime.GOARCH
	for _, a := range m.Artifacts {
		if a.GOOS != runtime.GOOS || a.GOARCH != runtime.GOARCH || !artifactCompatible(ctx, a) {
			continue
		}
		err := g.asset(ctx, stage, a, out)
		if errors.Is(err, errAssetMissing) {
			break
		}
		if err != nil {
			return err
		}
		pin.Source, pin.Release, pin.Asset = "prebuilt", a.Release, a.Asset
		return finishExecutable(out, pin)
	}
	if !build {
		return errors.New("no matching GitHub release asset; use --build to build the plugin")
	}
	root, err := os.OpenRoot(work)
	if err != nil {
		return err
	}
	defer func() { _ = root.Close() }()
	_, err = readFile(root, "go.mod", 1<<20)
	if err != nil {
		return errors.New("plugin go.mod does not match its manifest")
	}
	sum, err := readFile(root, "go.sum", 4<<20)
	if err != nil {
		return errors.New("plugin build requires go.sum")
	}
	run := func(args ...string) *exec.Cmd {
		cmd := exec.CommandContext(ctx, "go", args...)
		cmd.Dir = work
		cmd.Env = append(os.Environ(), "GOWORK=off", "GOFLAGS=", "GOTOOLCHAIN=local", "GOENV=off")
		return cmd
	}
	var mod struct {
		Module  struct{ Path string }
		Replace []struct{ New struct{ Version string } }
	}
	metadata, err := run("mod", "edit", "-json").Output()
	if err != nil {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		return errors.New("plugin go.mod does not match its manifest")
	}
	if json.Unmarshal(metadata, &mod) != nil || mod.Module.Path != m.Module {
		return errors.New("plugin go.mod does not match its manifest")
	}
	for _, replacement := range mod.Replace {
		if replacement.New.Version == "" {
			return errors.New("plugin build does not allow workspace module replacements")
		}
	}
	version, err := run("version").Output()
	if err != nil {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		return errors.New("plugin build requires a local Go toolchain")
	}
	if err := run("build", "-mod=readonly", "-o", out, m.Package).Run(); err != nil {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		return errors.New("plugin source build failed")
	}
	pin.Source, pin.Toolchain, pin.ModuleSum = "source", strings.TrimSpace(string(version)), digest(sum)
	return finishExecutable(out, pin)
}

func (g githubRepo) asset(ctx context.Context, stage string, a Artifact, out string) error {
	body, err := fetch(ctx, g.location+"/releases/download/"+url.PathEscape(a.Release)+"/"+url.PathEscape(a.Asset), archiveLimit)
	if err != nil {
		return err
	}
	defer func() { _ = body.Close() }()
	raw, err := io.ReadAll(body)
	if err != nil {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		return errors.New("GitHub plugin asset digest or size mismatch")
	}
	if len(raw) > archiveLimit || digest(raw) != a.SHA256 {
		return errors.New("GitHub plugin asset digest or size mismatch")
	}
	dest := filepath.Join(stage, "asset")
	if err := extractTarGzip(ctx, bytes.NewReader(raw), dest, false); err != nil {
		return err
	}
	info, err := os.Stat(filepath.Join(dest, a.Executable))
	if err != nil || !info.Mode().IsRegular() {
		return errors.New("plugin executable is missing from the asset")
	}
	return copyPluginFile(filepath.Join(dest, a.Executable), out, 0700)
}

func artifactCompatible(ctx context.Context, a Artifact) bool {
	// Library requirements have no portable probe. Only self-contained builds
	// are selected automatically.
	if len(a.RuntimeLibraries) != 0 {
		return false
	}
	if a.MinimumOS == "" {
		return true
	}
	if runtime.GOOS != "darwin" {
		return false
	}
	version, err := exec.CommandContext(ctx, "/usr/bin/sw_vers", "-productVersion").Output()
	return err == nil && versionAtLeast(strings.TrimSpace(string(version)), a.MinimumOS)
}

func finishExecutable(path string, pin *installedPlugin) error {
	if !nativeExecutable(path) {
		return errors.New("plugin executable has an unsupported format or architecture")
	}
	root, err := os.OpenRoot(filepath.Dir(path))
	if err != nil {
		return err
	}
	defer func() { _ = root.Close() }()
	raw, err := readFile(root, filepath.Base(path), archiveLimit)
	if err != nil {
		return errors.New("plugin executable exceeds the size limit")
	}
	pin.Digest = digest(raw)
	return nil
}

func nativeExecutable(path string) bool {
	switch runtime.GOOS {
	case "darwin":
		want := map[string]macho.Cpu{"amd64": macho.CpuAmd64, "arm64": macho.CpuArm64}[runtime.GOARCH]
		f, err := macho.Open(path)
		if err == nil {
			defer func() { _ = f.Close() }()
			return f.Type == macho.TypeExec && f.Cpu == want
		}
		fat, err := macho.OpenFat(path)
		if err != nil {
			return false
		}
		defer func() { _ = fat.Close() }()
		for _, arch := range fat.Arches {
			if arch.Type == macho.TypeExec && arch.Cpu == want {
				return true
			}
		}
	case "linux":
		f, err := elf.Open(path)
		if err != nil {
			return false
		}
		defer func() { _ = f.Close() }()
		want := map[string]elf.Machine{"amd64": elf.EM_X86_64, "arm64": elf.EM_AARCH64, "386": elf.EM_386, "arm": elf.EM_ARM}[runtime.GOARCH]
		return want != 0 && f.Entry != 0 && f.Machine == want && (f.Type == elf.ET_EXEC || f.Type == elf.ET_DYN)
	case "windows":
		f, err := pe.Open(path)
		if err != nil {
			return false
		}
		defer func() { _ = f.Close() }()
		want := map[string]uint16{"amd64": pe.IMAGE_FILE_MACHINE_AMD64, "arm64": pe.IMAGE_FILE_MACHINE_ARM64, "386": pe.IMAGE_FILE_MACHINE_I386}[runtime.GOARCH]
		return want != 0 && f.Machine == want && f.Characteristics&pe.IMAGE_FILE_EXECUTABLE_IMAGE != 0 && f.Characteristics&pe.IMAGE_FILE_DLL == 0
	}
	return false
}

func versionAtLeast(actual, minimum string) bool {
	parts := func(s string) ([]int, bool) {
		var out []int
		for _, part := range strings.Split(s, ".") {
			n, err := strconv.Atoi(part)
			if err != nil || n < 0 {
				return nil, false
			}
			out = append(out, n)
		}
		return out, true
	}
	a, aOK := parts(actual)
	b, bOK := parts(minimum)
	if !aOK || !bOK {
		return false
	}
	for i := range max(len(a), len(b)) {
		var x, y int
		if i < len(a) {
			x = a[i]
		}
		if i < len(b) {
			y = b[i]
		}
		if x != y {
			return x > y
		}
	}
	return true
}
