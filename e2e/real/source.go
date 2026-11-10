package main

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
)

type sourceIdentity struct {
	Head     string            `json:"head"`
	Files    map[string]string `json:"sha256"`
	BuildVCS bool              `json:"build_vcs"`
	CGO      bool              `json:"cgo"`
	Links    map[string]string `json:"materialized_links,omitempty"`
}

func command(ctx context.Context, dir string, env []string, name string, args ...string) ([]byte, error) {
	cmd := exec.CommandContext(ctx, name, args...)
	cmd.Dir = dir
	if env != nil {
		cmd.Env = env
	}
	b, err := cmd.CombinedOutput()
	if err != nil {
		return b, fmt.Errorf("%s command failed", filepath.Base(name))
	}
	return b, nil
}
func sourceFiles(ctx context.Context, root string) ([]string, error) {
	b, err := command(ctx, root, nil, "git", "ls-files", "-z")
	if err != nil {
		return nil, err
	}
	files := map[string]bool{}
	for _, f := range strings.Split(string(b), "\x00") {
		if f != "" {
			files[f] = true
		}
	}
	b, err = command(ctx, root, nil, "git", "ls-files", "-z", "--others", "--exclude-standard", "--", "engine", "internal", "plugin", "plugins", "app/service", "e2e/real")
	if err != nil {
		return nil, err
	}
	for _, f := range strings.Split(string(b), "\x00") {
		if f != "" {
			files[f] = true
		}
	}
	out := []string{}
	for f := range files {
		if strings.Contains(f, "/__pycache__/") {
			continue
		}
		out = append(out, f)
	}
	sort.Strings(out)
	return out, nil
}
func freezeSource(ctx context.Context, root, dest string) (sourceIdentity, error) {
	result := sourceIdentity{Files: map[string]string{}, Links: map[string]string{}}
	b, err := command(ctx, root, nil, "git", "rev-parse", "HEAD")
	if err != nil {
		return result, err
	}
	result.Head = strings.TrimSpace(string(b))
	files, err := sourceFiles(ctx, root)
	if err != nil {
		return result, err
	}
	for _, name := range files {
		path := filepath.Join(root, filepath.FromSlash(name))
		info, err := os.Lstat(path)
		if os.IsNotExist(err) {
			result.Files[name] = "deleted"
			continue
		}
		if err != nil {
			return result, fmt.Errorf("source_stat_failed")
		}
		if info.Mode()&os.ModeSymlink != 0 {
			resolved, err := filepath.EvalSymlinks(path)
			if err != nil {
				return result, fmt.Errorf("source_link_resolution_failed")
			}
			relative, err := filepath.Rel(root, resolved)
			if err != nil || relative == ".." || strings.HasPrefix(relative, ".."+string(filepath.Separator)) {
				return result, fmt.Errorf("source_link_leaves_repository")
			}
			result.Links[name] = filepath.ToSlash(relative)
			info, err = os.Stat(resolved)
			if err != nil {
				return result, fmt.Errorf("source_link_stat_failed")
			}
		}
		if !info.Mode().IsRegular() {
			return result, fmt.Errorf("source_requires_regular_files: %s", name)
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return result, fmt.Errorf("source_read_failed")
		}
		result.Files[name] = digest(data)
		to := filepath.Join(dest, filepath.FromSlash(name))
		if err = os.MkdirAll(filepath.Dir(to), 0700); err != nil {
			return result, err
		}
		if err = os.WriteFile(to, data, info.Mode().Perm()&0777); err != nil {
			return result, err
		}
	}
	again, err := sourceFiles(ctx, root)
	if err != nil || strings.Join(files, "\x00") != strings.Join(again, "\x00") {
		return result, fmt.Errorf("source_changed_while_snapshotting")
	}
	for name, sha := range result.Files {
		b, err := os.ReadFile(filepath.Join(root, filepath.FromSlash(name)))
		if sha == "deleted" && os.IsNotExist(err) {
			continue
		}
		if err != nil || digest(b) != sha {
			return result, fmt.Errorf("source_changed_while_snapshotting")
		}
	}
	return result, nil
}
func checkSnapshot(root string, s sourceIdentity) error {
	for name, sha := range s.Files {
		b, err := os.ReadFile(filepath.Join(root, filepath.FromSlash(name)))
		if sha == "deleted" && os.IsNotExist(err) {
			continue
		}
		if err != nil || digest(b) != sha {
			return fmt.Errorf("frozen_source_changed")
		}
	}
	return nil
}
func buildTarget(ctx context.Context, source, dir, osName, arch string, products bool) (map[string]string, error) {
	if err := os.MkdirAll(dir, 0700); err != nil {
		return nil, err
	}
	env := []string{}
	for _, s := range os.Environ() {
		if !strings.HasPrefix(s, "GOOS=") && !strings.HasPrefix(s, "GOARCH=") && !strings.HasPrefix(s, "CGO_ENABLED=") && !strings.HasPrefix(s, "GOFLAGS=") && !strings.HasPrefix(s, "GOWORK=") {
			env = append(env, s)
		}
	}
	env = append(env, "GOOS="+osName, "GOARCH="+arch, "CGO_ENABLED=0", "GOFLAGS=", "GOWORK=off")
	suffix := ""
	if osName == "windows" {
		suffix = ".exe"
	}
	targets := map[string]string{"unic-e2e-worker": "./e2e/real"}
	if products {
		targets["unic"] = "./app/service/cmd/unic"
		targets["unicd"] = "./app/service/cmd/unicd"
	}
	hashes := map[string]string{}
	for _, name := range []string{"unic-e2e-worker", "unic", "unicd"} {
		pkg, ok := targets[name]
		if !ok {
			continue
		}
		path := filepath.Join(dir, name+suffix)
		if b, err := command(ctx, source, env, "go", "build", "-mod=readonly", "-trimpath", "-buildvcs=false", "-o", path, pkg); err != nil {
			_ = os.WriteFile(filepath.Join(dir, "build-"+name+".log"), b, 0600)
			return nil, fmt.Errorf("build_failed_%s_%s_%s", osName, arch, name)
		}
		sha, _, err := fileSHA(path)
		if err != nil {
			return nil, err
		}
		hashes[name+suffix] = sha
	}
	return hashes, nil
}
