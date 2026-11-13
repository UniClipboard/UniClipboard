// Command package builds reusable headless binaries and optional desktop archives.
package main

import (
	"archive/tar"
	"archive/zip"
	"compress/gzip"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"regexp"
	"runtime"
	"strings"
	"time"

	"github.com/BurntSushi/toml"
)

func main() {
	if e := run(); e != nil {
		fmt.Fprintln(os.Stderr, e)
		os.Exit(1)
	}
}
func run() error {
	desktop := flag.Bool("desktop", false, "also build the desktop package")
	output := flag.String("output", "dist/releases", "output directory")
	product := flag.String("product", "", "versioned product candidate: cli or desktop (empty keeps development archive layout)")
	metadata := flag.String("metadata", "release/products.toml", "product metadata file")
	cliManifest := flag.String("cli-manifest", "", "accepted immutable CLI manifest for a Desktop candidate")
	target := flag.String("target", "", "Desktop target OS-architecture")
	development := flag.Bool("development", false, "allow dirty source for a non-release Desktop candidate")
	flag.Parse()
	if *product == "desktop" {
		if *desktop || *cliManifest == "" || *target == "" {
			return fmt.Errorf("Desktop candidate requires -cli-manifest and -target, without -desktop")
		}
		python := "python3"
		if runtime.GOOS == "windows" {
			python = "python"
		}
		args := []string{"tools/package/desktop.py", "stage", "--metadata", *metadata, "--cli-manifest", *cliManifest, "--target", *target, "--output", *output}
		if *development {
			args = append(args, "--development")
		}
		cmd := exec.Command(python, args...)
		cmd.Stdout, cmd.Stderr = os.Stdout, os.Stderr
		return cmd.Run()
	}
	if *product != "" && *product != "cli" {
		return fmt.Errorf("unsupported product: %s", *product)
	}
	if *product != "" && *desktop {
		return fmt.Errorf("a CLI candidate cannot include a desktop build")
	}
	buildNumber := 0
	productVersion := ""
	metadataDigest := ""
	if *product != "" {
		var config struct {
			Schema   int
			Products map[string]struct {
				Version     string
				BuildNumber int `toml:"build_number"`
				Targets     []string
			}
		}
		data, err := os.ReadFile(*metadata)
		if err != nil {
			return err
		}
		sum := sha256.Sum256(data)
		metadataDigest = hex.EncodeToString(sum[:])
		if _, err := toml.Decode(string(data), &config); err != nil {
			return err
		}
		p, ok := config.Products[*product]
		if !ok || config.Schema != 1 || !regexp.MustCompile(`^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$`).MatchString(p.Version) || p.BuildNumber < 1 || p.BuildNumber > 999999999 {
			return fmt.Errorf("invalid product build metadata")
		}
		buildNumber, productVersion = p.BuildNumber, p.Version
	}
	// Build-time metadata only. No runtime environment variable enables upload
	// or overrides the selected project. Never print the ingestion token.
	phHost := os.Getenv("UNIC_POSTHOG_HOST")
	phToken := os.Getenv("UNIC_POSTHOG_PROJECT_TOKEN")
	version := os.Getenv("UNIC_PRODUCT_VERSION")
	if productVersion != "" {
		if version != "" && version != productVersion {
			return fmt.Errorf("UNIC_PRODUCT_VERSION conflicts with product metadata")
		}
		version = productVersion
	}
	if (phHost == "") != (phToken == "") {
		return fmt.Errorf("PostHog release host and project token must be supplied together")
	}
	if phHost != "" && phHost != "https://us.i.posthog.com" && phHost != "https://eu.i.posthog.com" {
		return fmt.Errorf("PostHog release host must select US or EU Cloud")
	}
	if phToken != "" && !regexp.MustCompile(`^phc_[A-Za-z0-9_-]+$`).MatchString(phToken) {
		return fmt.Errorf("PostHog release credential must be an ingestion-only project token")
	}
	if version == "" {
		version = "dev"
	}
	if !regexp.MustCompile(`^[A-Za-z0-9._+-]{1,64}$`).MatchString(version) {
		return fmt.Errorf("invalid product version")
	}
	linker := "-X github.com/uniclipboard/unic/internal/observability.ProductVersion=" + version
	head, err := exec.Command("git", "rev-parse", "HEAD").Output()
	if err != nil {
		return err
	}
	initialDirty, err := exec.Command("git", "status", "--porcelain").Output()
	if err != nil {
		return err
	}
	linker += fmt.Sprintf(" -X github.com/uniclipboard/unic/internal/releaseinfo.Version=%s -X github.com/uniclipboard/unic/internal/releaseinfo.BuildNumber=%d -X github.com/uniclipboard/unic/internal/releaseinfo.Source=%s", version, buildNumber, strings.TrimSpace(string(head)))
	if phHost != "" {
		linker += " -X github.com/uniclipboard/unic/internal/observability.PostHogHost=" + phHost + " -X github.com/uniclipboard/unic/internal/observability.PostHogToken=" + phToken
	}
	targetOS := os.Getenv("GOOS")
	if targetOS == "" {
		targetOS = runtime.GOOS
	}
	arch := os.Getenv("GOARCH")
	if arch == "" {
		arch = runtime.GOARCH
	}
	suffix := ""
	if targetOS == "windows" {
		suffix = ".exe"
	}
	if e := os.MkdirAll(*output, 0755); e != nil {
		return e
	}
	work, e := os.MkdirTemp("", "unic-package")
	if e != nil {
		return e
	}
	defer func() { _ = os.RemoveAll(work) }()
	names := []string{"unic", "unicd"}
	if *desktop {
		names = append(names, "unic-desktop")
	}
	source, err := sourceHashes(names)
	if err != nil {
		return err
	}
	archives := map[string]string{}
	signingIdentity := os.Getenv("UNIC_MACOS_SIGNING_IDENTITY")
	windowsCertificate := os.Getenv("UNIC_WINDOWS_CERT_SHA1")
	signed := (targetOS == "darwin" && signingIdentity != "") || (targetOS == "windows" && windowsCertificate != "")
	if windowsCertificate != "" && !regexp.MustCompile(`^[a-fA-F0-9]{40}$`).MatchString(windowsCertificate) {
		return fmt.Errorf("invalid Windows signing certificate thumbprint")
	}
	hashes := map[string]string{}
	for _, name := range names {
		app := "service"
		if name == "unic-desktop" {
			app = "desktop"
		}
		path := filepath.Join(work, name+suffix)
		cmd := exec.Command("go", "build", "-trimpath", "-buildvcs=false", "-ldflags", linker, "-o", path, "./app/"+app+"/cmd/"+name)
		cmd.Stdout, cmd.Stderr = os.Stdout, os.Stderr
		if e = cmd.Run(); e != nil {
			return e
		}
		var signCommands [][]string
		if targetOS == "darwin" && signingIdentity != "" {
			signCommands = [][]string{{"codesign", "--force", "--options", "runtime", "--timestamp", "--sign", signingIdentity, path}, {"codesign", "--verify", "--strict", path}}
		} else if targetOS == "windows" && windowsCertificate != "" {
			signCommands = [][]string{{"signtool", "sign", "/fd", "SHA256", "/td", "SHA256", "/tr", "http://timestamp.digicert.com", "/sha1", windowsCertificate, path}, {"signtool", "verify", "/pa", path}}
		}
		for _, command := range signCommands {
			cmd := exec.Command(command[0], command[1:]...)
			cmd.Stdout, cmd.Stderr = os.Stdout, os.Stderr
			if e = cmd.Run(); e != nil {
				return fmt.Errorf("sign executable before CLI manifest acceptance: %w", e)
			}
		}
		f, e := os.Open(path)
		if e != nil {
			return e
		}
		h := sha256.New()
		_, e = io.Copy(h, f)
		_ = f.Close()
		if e != nil {
			return e
		}
		hashes[name+suffix] = hex.EncodeToString(h.Sum(nil))
	}
	alias, aliasTarget := "uniclip", "unic"
	if targetOS == "windows" {
		alias, aliasTarget = "uniclip.cmd", "unic.exe"
		e = os.WriteFile(filepath.Join(work, alias), []byte("@echo off\r\n\"%~dp0unic.exe\" %*\r\nexit /b %errorlevel%\r\n"), 0644)
	} else {
		e = os.Symlink(aliasTarget, filepath.Join(work, alias))
	}
	if e != nil {
		return e
	}
	for _, kind := range []string{"headless", "desktop"} {
		if kind == "desktop" && !*desktop {
			continue
		}
		files := []string{"unic" + suffix, "unicd" + suffix, alias}
		if kind == "desktop" {
			files = append(files, "unic-desktop"+suffix)
		}
		base := "unic-" + kind + "-" + targetOS + "-" + arch
		if *product != "" {
			base = fmt.Sprintf("unic-%s-%s-build.%d-%s-%s", *product, version, buildNumber, targetOS, arch)
		}
		ext := ".tar.gz"
		if targetOS == "windows" {
			ext = ".zip"
		}
		path := filepath.Join(*output, base+ext)
		if e = archive(path, work, files, targetOS == "windows"); e != nil {
			return e
		}
		b, e := os.ReadFile(path)
		if e != nil {
			return e
		}
		sum := sha256.Sum256(b)
		archives[filepath.Base(path)] = hex.EncodeToString(sum[:])
		fmt.Println(path)
	}
	dirty, err := exec.Command("git", "status", "--porcelain").Output()
	if err != nil {
		return err
	}
	finalHead, err := exec.Command("git", "rev-parse", "HEAD").Output()
	if err != nil {
		return err
	}
	if string(head) != string(finalHead) {
		return fmt.Errorf("source revision changed during packaging")
	}
	after, e := sourceHashes(names)
	if e != nil {
		return e
	}
	if !reflect.DeepEqual(source, after) {
		return fmt.Errorf("source changed during packaging")
	}
	manifest := map[string]any{"source_sha256": source, "source_unchanged": true, "archives_sha256": archives, "os": targetOS, "arch": arch, "go": runtime.Version(), "source_head": strings.TrimSpace(string(head)), "source_dirty": len(initialDirty) > 0 || len(dirty) > 0, "binaries_sha256": hashes, "aliases": map[string]string{alias: aliasTarget}, "desktop": *desktop, "signed": signed, "telemetry_host": phHost, "ingestion_token_configured": phToken != "", "product_version": version}
	if *product != "" {
		manifest["product"], manifest["build_number"] = *product, buildNumber
		data, e := os.ReadFile(*metadata)
		if e != nil {
			return e
		}
		sum := sha256.Sum256(data)
		if hex.EncodeToString(sum[:]) != metadataDigest {
			return fmt.Errorf("product metadata changed during packaging")
		}
		manifest["metadata_sha256"] = metadataDigest
	}
	data, e := json.MarshalIndent(manifest, "", "  ")
	if e != nil {
		return e
	}
	return os.WriteFile(filepath.Join(*output, "manifest-"+targetOS+"-"+arch+".json"), data, 0644)
}
func archive(dest, dir string, names []string, asZip bool) (err error) {
	f, err := os.Create(dest)
	if err != nil {
		return err
	}
	defer func() {
		if e := f.Close(); err == nil {
			err = e
		}
		if err != nil {
			_ = os.Remove(dest)
		}
	}()
	if asZip {
		z := zip.NewWriter(f)
		for _, name := range names {
			in, e := os.Open(filepath.Join(dir, name))
			if e != nil {
				return e
			}
			info, e := in.Stat()
			if e != nil {
				_ = in.Close()
				return e
			}
			h, e := zip.FileInfoHeader(info)
			if e != nil {
				_ = in.Close()
				return e
			}
			h.Name = name
			h.Modified = time.Date(1980, 1, 1, 0, 0, 0, 0, time.UTC)
			w, e := z.CreateHeader(h)
			if e == nil {
				_, e = io.Copy(w, in)
			}
			_ = in.Close()
			if e != nil {
				return e
			}
		}
		return z.Close()
	}
	gz := gzip.NewWriter(f)
	tw := tar.NewWriter(gz)
	for _, name := range names {
		path := filepath.Join(dir, name)
		info, e := os.Lstat(path)
		if e != nil {
			return e
		}
		link := ""
		if info.Mode()&os.ModeSymlink != 0 {
			link, e = os.Readlink(path)
			if e != nil {
				return e
			}
		}
		h, e := tar.FileInfoHeader(info, link)
		if e != nil {
			return e
		}
		h.Name = name
		h.ModTime = time.Unix(0, 0)
		h.Uid = 0
		h.Gid = 0
		h.Uname = ""
		h.Gname = ""
		if e = tw.WriteHeader(h); e != nil {
			return e
		}
		if info.Mode()&os.ModeSymlink != 0 {
			continue
		}
		in, e := os.Open(path)
		if e != nil {
			return e
		}
		_, e = io.Copy(tw, in)
		_ = in.Close()
		if e != nil {
			return e
		}
	}
	if err = tw.Close(); err != nil {
		return err
	}
	return gz.Close()
}

func sourceHashes(names []string) (map[string]string, error) {
	root, err := os.Getwd()
	if err != nil {
		return nil, err
	}
	args := []string{"list", "-deps", "-json"}
	for _, name := range names {
		app := "service"
		if name == "unic-desktop" {
			app = "desktop"
		}
		args = append(args, "./app/"+app+"/cmd/"+name)
	}
	cmd := exec.Command("go", args...)
	data, err := cmd.Output()
	if err != nil {
		return nil, err
	}
	decoder := json.NewDecoder(strings.NewReader(string(data)))
	paths := []string{"go.mod", "go.sum", "tools/package/main.go"}
	for {
		var pkg struct {
			Dir                                                                        string
			GoFiles, CgoFiles, CFiles, CXXFiles, HFiles, SFiles, SysoFiles, EmbedFiles []string
		}
		err = decoder.Decode(&pkg)
		if err == io.EOF {
			break
		}
		if err != nil {
			return nil, err
		}
		rel, err := filepath.Rel(root, pkg.Dir)
		if err != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(os.PathSeparator)) {
			continue
		}
		for _, group := range [][]string{pkg.GoFiles, pkg.CgoFiles, pkg.CFiles, pkg.CXXFiles, pkg.HFiles, pkg.SFiles, pkg.SysoFiles, pkg.EmbedFiles} {
			for _, name := range group {
				paths = append(paths, filepath.Join(rel, name))
			}
		}
	}
	hashes := map[string]string{}
	for _, path := range paths {
		data, err := os.ReadFile(path)
		if err != nil {
			return nil, err
		}
		sum := sha256.Sum256(data)
		hashes[filepath.ToSlash(path)] = hex.EncodeToString(sum[:])
	}
	return hashes, nil
}
