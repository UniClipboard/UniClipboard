package main

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/subtle"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/netip"
	"os"
	"os/exec"
	"regexp"
	"strings"
	"time"
)

var ipv4Pattern = regexp.MustCompile(`\b(\d{1,3}(?:\.\d{1,3}){3})\b`)

func parallelsExec(ctx context.Context, vm string, args ...string) ([]byte, error) {
	full := append([]string{"exec", vm, "--current-user"}, args...)
	cmd := exec.CommandContext(ctx, "prlctl", full...)
	var out bytes.Buffer
	cmd.Stdout = &out
	if err := cmd.Run(); err != nil {
		return nil, fmt.Errorf("parallels_command_failed")
	}
	return out.Bytes(), nil
}

func parallelsPause(ctx context.Context, vm string) error {
	if _, err := command(ctx, "", nil, "prlctl", "pause", vm); err != nil {
		return fmt.Errorf("parallels_pause_failed")
	}
	return nil
}

func parallelsResume(ctx context.Context, vm string) error {
	if _, err := command(ctx, "", nil, "prlctl", "resume", vm); err != nil {
		return fmt.Errorf("parallels_resume_failed")
	}
	return nil
}

// parallelsHostIP finds the local macOS bridge IPv4 address sharing a subnet
// with the running Parallels VM so the worker can fetch binaries and dial back
// over the private virtual NIC without SSH or shared folders.
func parallelsHostIP(ctx context.Context, vm string) (string, error) {
	out, err := command(ctx, "", nil, "prlctl", "list", "-i", vm)
	if err != nil {
		return "", fmt.Errorf("parallels_info_failed")
	}
	var guestIPs []netip.Addr
	for _, line := range strings.Split(string(out), "\n") {
		if !strings.Contains(strings.ToLower(line), "ip address") {
			continue
		}
		for _, m := range ipv4Pattern.FindAllString(line, -1) {
			if addr, err := netip.ParseAddr(m); err == nil && addr.Is4() && !addr.IsLoopback() {
				guestIPs = append(guestIPs, addr)
			}
		}
	}
	if len(guestIPs) == 0 {
		return "", fmt.Errorf("parallels_guest_ip_unavailable")
	}
	ifaces, err := net.InterfaceAddrs()
	if err != nil {
		return "", fmt.Errorf("parallels_host_interfaces_failed")
	}
	for _, ia := range ifaces {
		ipNet, ok := ia.(*net.IPNet)
		if !ok || ipNet.IP == nil {
			continue
		}
		addr, ok := netip.AddrFromSlice(ipNet.IP.To4())
		if !ok || !addr.Is4() || addr.IsLoopback() {
			continue
		}
		ones, _ := ipNet.Mask.Size()
		pfx := netip.PrefixFrom(addr, ones)
		for _, g := range guestIPs {
			if pfx.Contains(g) {
				return addr.String(), nil
			}
		}
	}
	return "", fmt.Errorf("parallels_bridge_ip_not_found")
}

func parallelsDetect(ctx context.Context, vm string) (platform, error) {
	probe, cancel := context.WithTimeout(ctx, 25*time.Second)
	defer cancel()
	if b, err := parallelsExec(probe, vm, "sh", "-c", "uname -s -m"); err == nil {
		parts := strings.Fields(string(b))
		if len(parts) == 2 {
			osName := map[string]string{"Darwin": "darwin", "Linux": "linux"}[parts[0]]
			arch := map[string]string{"arm64": "arm64", "aarch64": "arm64", "x86_64": "amd64", "amd64": "amd64"}[parts[1]]
			if osName != "" && arch != "" {
				return platform{OS: osName, Arch: arch}, nil
			}
		}
	}
	b, err := parallelsExec(probe, vm, "cmd", "/c", ps("[Console]::WriteLine([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString())"))
	if err != nil {
		return platform{}, fmt.Errorf("parallels_platform_probe_failed")
	}
	arch := map[string]string{"X64": "amd64", "Arm64": "arm64"}[strings.TrimSpace(string(b))]
	if arch == "" {
		return platform{}, fmt.Errorf("unsupported_windows_architecture")
	}
	return platform{OS: "windows", Arch: arch}, nil
}

func randomHex32() (string, error) {
	var buf [32]byte
	if _, err := rand.Read(buf[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(buf[:]), nil
}

// parallelsCopy transfers source from the macOS controller to dest inside the
// Parallels VM via a one-shot token-authenticated HTTP server bound strictly
// to the host's virtual bridge IP.
func (n *node) parallelsCopy(ctx context.Context, source, dest string) error {
	if n.hostIP == "" {
		ip, err := parallelsHostIP(ctx, n.config.VM)
		if err != nil {
			return err
		}
		n.hostIP = ip
	}
	tok, err := randomHex32()
	if err != nil {
		return err
	}
	ln, err := net.Listen("tcp", net.JoinHostPort(n.hostIP, "0"))
	if err != nil {
		return fmt.Errorf("parallels_copy_listen_failed")
	}
	defer func() { _ = ln.Close() }()

	srv := &http.Server{
		ReadHeaderTimeout: 5 * time.Second,
		Handler: http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			if subtle.ConstantTimeCompare([]byte(r.Header.Get("X-Unic-Token")), []byte(tok)) != 1 {
				http.Error(w, "forbidden", http.StatusForbidden)
				return
			}
			f, err := os.Open(source)
			if err != nil {
				http.Error(w, "missing", http.StatusNotFound)
				return
			}
			defer func() { _ = f.Close() }()
			w.Header().Set("Content-Type", "application/octet-stream")
			_, _ = io.Copy(w, f)
		}),
	}
	go func() { _ = srv.Serve(ln) }()
	defer func() {
		sctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		_ = srv.Shutdown(sctx)
		cancel()
	}()

	url := fmt.Sprintf("http://%s/artifact", ln.Addr().String())
	if n.platform.OS == "windows" {
		winDest := strings.ReplaceAll(dest, "/", "\\")
		script := fmt.Sprintf(
			"$d=%s; [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($d)) | Out-Null; "+
				"$ProgressPreference='SilentlyContinue'; "+
				"Invoke-WebRequest -UseBasicParsing -Uri %s -Headers @{'X-Unic-Token'=%s} -OutFile $d",
			quotePS(winDest), quotePS(url), quotePS(tok),
		)
		if _, err = parallelsExec(ctx, n.config.VM, "cmd", "/c", ps(script)); err != nil {
			return fmt.Errorf("parallels_artifact_copy_failed")
		}
		return nil
	}
	script := fmt.Sprintf("mkdir -p %s && curl -fsSL -H %s %s -o %s && chmod 700 %s",
		quoteSH(dest[:strings.LastIndex(dest, "/")]),
		quoteSH("X-Unic-Token: "+tok),
		quoteSH(url),
		quoteSH(dest),
		quoteSH(dest),
	)
	if _, err = parallelsExec(ctx, n.config.VM, "sh", "-c", script); err != nil {
		return fmt.Errorf("parallels_artifact_copy_failed")
	}
	return nil
}

type controlAuth struct {
	Token string `json:"token"`
}

func (n *node) bootstrapParallels(ctx context.Context, workerPath, runID string) (io.ReadCloser, io.WriteCloser, *exec.Cmd, error) {
	ip, err := parallelsHostIP(ctx, n.config.VM)
	if err != nil {
		return nil, nil, nil, err
	}
	n.hostIP = ip

	var b []byte
	if n.platform.OS == "windows" {
		script := "$p=Join-Path ([IO.Path]::GetTempPath()) ('unic-e2e-'+[Guid]::NewGuid().ToString('N')); [IO.Directory]::CreateDirectory($p) | Out-Null; [IO.File]::WriteAllText((Join-Path $p 'owner')," + quotePS(runID) + "); [Console]::WriteLine($p)"
		b, err = parallelsExec(ctx, n.config.VM, "cmd", "/c", ps(script))
	} else {
		b, err = parallelsExec(ctx, n.config.VM, "sh", "-c", "p=$(mktemp -d \"${TMPDIR:-/tmp}/unic-e2e-XXXXXXXX\") && printf %s "+quoteSH(runID)+" > \"$p/owner\" && printf '%s\\n' \"$p\"")
	}
	if err != nil {
		return nil, nil, nil, fmt.Errorf("bootstrap_directory_failed")
	}
	n.platform.Bootstrap = strings.TrimSpace(string(b))
	if n.platform.Bootstrap == "" || strings.ContainsAny(n.platform.Bootstrap, "\r\n") {
		return nil, nil, nil, fmt.Errorf("invalid_bootstrap_directory")
	}

	filename := "unic-e2e-worker"
	if n.platform.OS == "windows" {
		filename += ".exe"
	}
	remoteWorker := strings.ReplaceAll(n.platform.Bootstrap, "\\", "/") + "/" + filename
	if err = n.parallelsCopy(ctx, workerPath, remoteWorker); err != nil {
		return nil, nil, nil, err
	}

	tok, err := randomHex32()
	if err != nil {
		return nil, nil, nil, err
	}
	tokenFile := strings.ReplaceAll(n.platform.Bootstrap, "\\", "/") + "/control.token"
	if n.platform.OS == "windows" {
		winTok := strings.ReplaceAll(tokenFile, "/", "\\")
		script := fmt.Sprintf("[IO.File]::WriteAllText(%s, %s)", quotePS(winTok), quotePS(tok))
		if _, err = parallelsExec(ctx, n.config.VM, "cmd", "/c", ps(script)); err != nil {
			return nil, nil, nil, fmt.Errorf("parallels_token_write_failed")
		}
	} else {
		script := fmt.Sprintf("umask 077 && printf %%s %s > %s", quoteSH(tok), quoteSH(tokenFile))
		if _, err = parallelsExec(ctx, n.config.VM, "sh", "-c", script); err != nil {
			return nil, nil, nil, fmt.Errorf("parallels_token_write_failed")
		}
	}

	ln, err := net.Listen("tcp", net.JoinHostPort(n.hostIP, "0"))
	if err != nil {
		return nil, nil, nil, fmt.Errorf("parallels_control_listen_failed")
	}
	defer func() { _ = ln.Close() }()

	var cmd *exec.Cmd
	if n.platform.OS == "windows" {
		winWorker := strings.ReplaceAll(remoteWorker, "/", "\\")
		winTok := strings.ReplaceAll(tokenFile, "/", "\\")
		cmd = exec.Command("prlctl", "exec", n.config.VM, "--current-user",
			winWorker, "worker", "--run-id", runID, "--node", n.name,
			"--control-addr", ln.Addr().String(), "--control-token-file", winTok)
	} else {
		cmd = exec.Command("prlctl", "exec", n.config.VM, "--current-user",
			remoteWorker, "worker", "--run-id", runID, "--node", n.name,
			"--control-addr", ln.Addr().String(), "--control-token-file", tokenFile)
	}
	stderr, err := os.OpenFile(n.private, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0600)
	if err != nil {
		return nil, nil, nil, err
	}
	cmd.Stdout = stderr
	cmd.Stderr = stderr
	if err = cmd.Start(); err != nil {
		_ = stderr.Close()
		return nil, nil, nil, fmt.Errorf("worker_start_failed")
	}

	type accepted struct {
		conn net.Conn
		err  error
	}
	ch := make(chan accepted, 1)
	go func() {
		if tl, ok := ln.(*net.TCPListener); ok {
			_ = tl.SetDeadline(time.Now().Add(30 * time.Second))
		}
		for {
			conn, err := ln.Accept()
			if err != nil {
				ch <- accepted{nil, err}
				return
			}
			_ = conn.SetReadDeadline(time.Now().Add(5 * time.Second))
			line, rerr := readAuthLine(conn, 512)
			_ = conn.SetReadDeadline(time.Time{})
			var auth controlAuth
			if rerr != nil || json.Unmarshal(line, &auth) != nil || subtle.ConstantTimeCompare([]byte(auth.Token), []byte(tok)) != 1 {
				_ = conn.Close()
				continue
			}
			ch <- accepted{conn, nil}
			return
		}
	}()

	var conn net.Conn
	select {
	case res := <-ch:
		if res.err != nil {
			_ = cmd.Process.Kill()
			_ = stderr.Close()
			return nil, nil, nil, fmt.Errorf("parallels_control_accept_failed")
		}
		conn = res.conn
	case <-ctx.Done():
		_ = cmd.Process.Kill()
		_ = stderr.Close()
		return nil, nil, nil, ctx.Err()
	}
	n.controlConn = conn
	n.stderrFile = stderr
	return conn, conn, cmd, nil
}

func readAuthLine(r io.Reader, limit int) ([]byte, error) {
	var buf []byte
	var b [1]byte
	for len(buf) < limit {
		n, err := r.Read(b[:])
		if n > 0 {
			buf = append(buf, b[0])
			if b[0] == '\n' {
				return buf, nil
			}
		}
		if err != nil {
			return nil, err
		}
	}
	return nil, fmt.Errorf("auth_line_too_long")
}

func (n *node) removeParallelsBootstrap(ctx context.Context, runID string) error {
	dir := n.platform.Bootstrap
	if dir == "" {
		return nil
	}
	var err error
	if n.platform.OS == "windows" {
		script := ps("$p=" + quotePS(dir) + "; if ([IO.File]::ReadAllText((Join-Path $p 'owner')) -ne " + quotePS(runID) + ") { exit 1 }; Remove-Item -LiteralPath $p -Recurse -Force")
		_, err = parallelsExec(ctx, n.config.VM, "cmd", "/c", script)
	} else {
		script := "test \"$(cat " + quoteSH(dir+"/owner") + ")\" = " + quoteSH(runID) + " && rm -rf -- " + quoteSH(dir)
		_, err = parallelsExec(ctx, n.config.VM, "sh", "-c", script)
	}
	if err != nil {
		return fmt.Errorf("%s: bootstrap_cleanup_failed", n.name)
	}
	return nil
}
