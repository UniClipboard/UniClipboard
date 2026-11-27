package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"runtime"
	"sort"
	"strings"
	"sync"
	"sync/atomic"
	"syscall"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
)

type worker struct {
	injectedCrashes                      int
	directoryJob                         *directoryJob
	root, socket, socketDir, runID, name string
	client                               *api.Client
	fixtures                             map[string]fixtureData
	gate                                 *armRequest
	known                                map[string]bool
	daemon                               *exec.Cmd
	daemonDone                           chan struct{}
	daemonLog                            *os.File
	forced                               bool
	secrets                              [][]byte
	closed                               bool
	adapterSession                       string
	adapterEpoch                         string
	adapterSeq                           uint64
}

func workerMain(ctx context.Context, runID, name, controlAddr, tokenFile string) (resultErr error) {
	if !nodeLabel.MatchString(name) || len(runID) != 32 {
		return fmt.Errorf("invalid worker identity")
	}
	var in io.Reader = os.Stdin
	var out io.Writer = os.Stdout
	if controlAddr != "" {
		rawTok, err := os.ReadFile(tokenFile)
		_ = os.Remove(tokenFile)
		if err != nil {
			return fmt.Errorf("control_token_read_failed")
		}
		tok := strings.TrimSpace(string(rawTok))
		if tok == "" {
			return fmt.Errorf("control_token_empty")
		}
		conn, err := (&net.Dialer{Timeout: 15 * time.Second}).DialContext(ctx, "tcp", controlAddr)
		if err != nil {
			return fmt.Errorf("control_dial_failed")
		}
		defer func() { _ = conn.Close() }()
		if err = json.NewEncoder(conn).Encode(controlAuth{Token: tok}); err != nil {
			return fmt.Errorf("control_auth_write_failed")
		}
		in, out = conn, conn
	}
	root, err := os.MkdirTemp("", "ue-")
	if err != nil {
		return err
	}
	root, err = filepath.EvalSymlinks(root)
	if err != nil {
		return err
	}
	w := &worker{root: root, runID: runID, name: name, fixtures: map[string]fixtureData{}}
	defer func() {
		if !w.closed {
			if _, err := w.close(false); err != nil && resultErr == nil {
				resultErr = err
			}
		}
	}()
	for _, p := range []string{"bin", "state", "fixtures"} {
		if err = os.MkdirAll(filepath.Join(root, p), 0700); err != nil {
			return err
		}
	}
	w.socket = filepath.Join(root, "s")
	if runtime.GOOS != "windows" && len(w.socket) > 90 {
		w.socketDir, err = os.MkdirTemp("/tmp", "ue-")
		if err != nil {
			return err
		}
		w.socket = filepath.Join(w.socketDir, "s")
	}
	if len(w.socket) > 100 {
		return fmt.Errorf("temporary socket path exceeds platform limit")
	}
	w.client = api.NewClient(w.socket)
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()
	var processing atomic.Bool
	var lastActivity atomic.Int64
	lastActivity.Store(time.Now().UnixNano())
	go func() {
		ticker := time.NewTicker(5 * time.Second)
		defer ticker.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
				if !processing.Load() && time.Since(time.Unix(0, lastActivity.Load())) > 60*time.Second {
					cancel()
					return
				}
			}
		}
	}()
	incoming := make(chan request)
	go func() {
		defer close(incoming)
		defer cancel()
		dec := json.NewDecoder(in)
		for {
			var req request
			if err := dec.Decode(&req); err != nil {
				return
			}
			select {
			case incoming <- req:
			case <-ctx.Done():
				return
			}
		}
	}()
	enc := json.NewEncoder(out)
	for {
		var req request
		select {
		case <-ctx.Done():
			return nil
		case next, ok := <-incoming:
			if !ok {
				return nil
			}
			req = next
		}
		processing.Store(true)
		lastActivity.Store(time.Now().UnixNano())
		value, opErr := w.handle(ctx, req)
		lastActivity.Store(time.Now().UnixNano())
		processing.Store(false)
		reply := response{ID: req.ID}
		if opErr != nil {
			reply.Error = opErr.Error()
			if req.Method == "submit" {
				if safe, ok := value.(submitResult); ok {
					reply.Value, err = json.Marshal(safe)
					if err != nil {
						return err
					}
				}
			}
			if !regexp.MustCompile(`^[a-z0-9_]+$`).MatchString(reply.Error) {
				_ = os.WriteFile(filepath.Join(w.root, "worker-error.txt"), []byte(opErr.Error()), 0600)
				reply.Error = "worker_operation_failed"
			}
		} else {
			reply.Value, err = json.Marshal(value)
			if err != nil {
				return err
			}
		}
		if err = enc.Encode(reply); err != nil {
			return err
		}
		if req.Method == "close" {
			return opErr
		}
	}
}
func decodeValue[T any](v json.RawMessage) (T, error) {
	var out T
	err := json.Unmarshal(v, &out)
	return out, err
}
func (w *worker) env() []string {
	env := []string{}
	for _, s := range os.Environ() {
		if !strings.HasPrefix(s, "UNIC_") {
			env = append(env, s)
		}
	}
	return append(env, "UNIC_HOME="+filepath.Join(w.root, "state"), "UNIC_SOCK="+w.socket)
}
func exe(name string) string {
	if runtime.GOOS == "windows" {
		return name + ".exe"
	}
	return name
}
func (w *worker) cli(ctx context.Context, args []string, stdin io.Reader) (cliResult, error) {
	ctx, cancel := context.WithTimeout(ctx, 90*time.Second)
	defer cancel()
	cmd := exec.CommandContext(ctx, filepath.Join(w.root, "bin", exe("unic")), args...)
	cmd.Env = w.env()
	cmd.Dir = w.root
	cmd.Stdin = stdin
	var out, stderr bytes.Buffer
	cmd.Stdout = &out
	cmd.Stderr = &stderr
	err := cmd.Run()
	result := cliResult{Out: out.Bytes(), Err: stderr.Bytes()}
	if err != nil {
		if exit, ok := err.(*exec.ExitError); ok {
			result.Code = exit.ExitCode()
		} else {
			return result, fmt.Errorf("cli_execution_failed")
		}
	}
	if len(args) > 1 && args[0] == "device" && args[1] == "addr" && result.Code == 0 {
		w.secrets = append(w.secrets, bytes.TrimSpace(result.Out))
	}
	if len(args) > 2 && args[0] == "device" && args[1] == "add" {
		w.secrets = append(w.secrets, []byte(args[2]))
	}
	return result, nil
}
func (w *worker) checkedCLI(ctx context.Context, args ...string) ([]byte, error) {
	v, err := w.cli(ctx, args, nil)
	if err != nil {
		return nil, err
	}
	if v.Code != 0 {
		return nil, fmt.Errorf("cli_%s_failed", args[0])
	}
	return v.Out, nil
}
func (w *worker) start(ctx context.Context) error {
	if w.daemon != nil {
		return fmt.Errorf("daemon_already_owned")
	}
	log, err := os.OpenFile(filepath.Join(w.root, "daemon.log"), os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0600)
	if err != nil {
		return fmt.Errorf("log_open_failed")
	}
	cmd := exec.Command(filepath.Join(w.root, "bin", exe("unicd")))
	cmd.Env = w.env()
	cmd.Dir = w.root
	cmd.Stdout = log
	cmd.Stderr = log
	if err = cmd.Start(); err != nil {
		_ = log.Close()
		return fmt.Errorf("daemon_start_failed")
	}
	w.daemon = cmd
	w.daemonLog = log
	w.daemonDone = make(chan struct{})
	go func() { _ = cmd.Wait(); close(w.daemonDone) }()
	deadline := time.Now().Add(60 * time.Second)
	for time.Now().Before(deadline) {
		select {
		case <-w.daemonDone:
			return fmt.Errorf("daemon_exited")
		case <-ctx.Done():
			return ctx.Err()
		default:
		}
		probe, cancel := context.WithTimeout(ctx, time.Second)
		_, err = w.client.Status(probe)
		cancel()
		if err == nil {
			return nil
		}
		time.Sleep(100 * time.Millisecond)
	}
	return fmt.Errorf("daemon_ready_timeout")
}
func (w *worker) stop() error {
	if w.daemon == nil {
		return nil
	}
	w.closeAdapter()
	select {
	case <-w.daemonDone:
	default:
		// The existing service API shuts down an observed generation. This
		// permits a cooperative stop on Windows without killing UDP state.
		stopCtx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		instance, serviceErr := w.client.Service(stopCtx)
		requested := false
		if serviceErr == nil {
			if instance.PID != w.daemon.Process.Pid {
				cancel()
				return fmt.Errorf("owned_service_identity_mismatch")
			}
			requested = w.client.StopService(stopCtx, instance.ID) == nil
		}
		cancel()
		exited := false
		if requested {
			select {
			case <-w.daemonDone:
				exited = true
			case <-time.After(5 * time.Second):
			}
		}
		if !exited {
			if runtime.GOOS == "windows" {
				w.forced = true
				_ = w.daemon.Process.Kill()
			} else {
				_ = w.daemon.Process.Signal(syscall.SIGTERM)
			}
		}
		select {
		case <-w.daemonDone:
		case <-time.After(15 * time.Second):
			w.forced = true
			_ = w.daemon.Process.Kill()
			select {
			case <-w.daemonDone:
			case <-time.After(5 * time.Second):
				return fmt.Errorf("daemon_stop_timeout")
			}
		}
	}
	if err := w.daemonLog.Close(); err != nil {
		return fmt.Errorf("daemon_log_close_failed")
	}
	w.daemon = nil
	w.daemonLog = nil
	verify, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	if _, err := w.client.Status(verify); err == nil {
		return fmt.Errorf("service_still_available_after_owned_exit")
	}
	return nil
}
func (w *worker) close(keep bool) (logsResult, error) {
	if w.directoryJob != nil {
		w.directoryJob.cancel()
		wait, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		_, err := w.finishDirectoryJob(wait)
		cancel()
		if err != nil {
			return logsResult{}, fmt.Errorf("directory_cli_cleanup_failed")
		}
	}

	if err := w.stop(); err != nil {
		return logsResult{}, err
	}
	b, err := os.ReadFile(filepath.Join(w.root, "daemon.log"))
	if os.IsNotExist(err) {
		b = nil
	} else if err != nil {
		return logsResult{}, fmt.Errorf("log_read_failed")
	}
	result := logsResult{InjectedCrashes: w.injectedCrashes, Bytes: len(b), SHA: digest(b), ForcedStop: w.forced}
	for _, secret := range w.secrets {
		if len(secret) > 0 && bytes.Contains(b, secret) {
			result.ContainsSecret = true
		}
	}
	if !keep {
		if err = os.RemoveAll(w.root); err != nil {
			return result, fmt.Errorf("workspace_cleanup_failed")
		}
		if w.socketDir != "" {
			if err = os.RemoveAll(w.socketDir); err != nil {
				return result, fmt.Errorf("socket_cleanup_failed")
			}
		}
		result.Removed = true
	}
	w.closed = true
	if result.ContainsSecret {
		return result, fmt.Errorf("secret_in_daemon_log")
	}
	return result, nil
}
func (w *worker) configure(ctx context.Context, v installConfig) error {
	if v.Name != w.name {
		return fmt.Errorf("node_identity_mismatch")
	}
	for name, sha := range v.Hashes {
		if name != exe("unic") && name != exe("unicd") {
			return fmt.Errorf("unexpected_program")
		}
		got, _, err := fileSHA(filepath.Join(w.root, "bin", name))
		if err != nil || got != sha {
			return fmt.Errorf("program_hash_mismatch")
		}
	}
	// Complete the production CLI's explicit no-source initialization once in an
	// isolated empty discovery home so the harness never inspects a user's legacy v1 profile.
	if err := w.initializeEmptyProfile(ctx); err != nil {
		return err
	}
	for name, b := range v.Plugins {
		clean := filepath.Clean(filepath.FromSlash(name))
		if filepath.IsAbs(clean) || clean == ".." || strings.HasPrefix(clean, ".."+string(filepath.Separator)) {
			return fmt.Errorf("unsafe_plugin_path")
		}
		path := filepath.Join(w.root, "state", "plugins", "lua", clean)
		if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
			return fmt.Errorf("plugin_directory_failed")
		}
		if err := os.WriteFile(path, b, 0600); err != nil {
			return fmt.Errorf("plugin_write_failed")
		}
	}
	config := fmt.Sprintf("device_name = %q\n[telemetry]\nenabled = false\n[plugins]\nlua = [\"example.lua-sensitive\", \"example.lua-emailtag\"]\n[plugins.instances.\"builtin.security\"]\nenabled = false\n[plugins.instances.\"example.lua-emailtag\".config]\ntag = %q\n", "lab-"+w.name, "local-"+w.name)
	if err := os.WriteFile(filepath.Join(w.root, "state", "config.toml"), []byte(config), 0600); err != nil {
		return fmt.Errorf("configuration_write_failed")
	}
	return nil
}
func (w *worker) initializeEmptyProfile(ctx context.Context) error {
	discovery, err := os.MkdirTemp(w.root, "init-discovery-")
	if err != nil {
		return fmt.Errorf("profile_init_dir_failed")
	}
	defer func() { _ = os.RemoveAll(discovery) }()
	initCtx, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	stateDir := filepath.Join(w.root, "state")
	cmd := exec.CommandContext(initCtx, filepath.Join(w.root, "bin", exe("unic")), "migrate", "--initialize-empty", "--target", stateDir, "--json")
	var env []string
	for _, s := range w.env() {
		upper := strings.ToUpper(s)
		switch {
		case strings.HasPrefix(upper, "HOME="),
			strings.HasPrefix(upper, "USERPROFILE="),
			strings.HasPrefix(upper, "APPDATA="),
			strings.HasPrefix(upper, "LOCALAPPDATA="),
			strings.HasPrefix(upper, "XDG_DATA_HOME="),
			strings.HasPrefix(upper, "XDG_CONFIG_HOME="),
			strings.HasPrefix(upper, "UC_PROFILE="):
			continue
		}
		env = append(env, s)
	}
	env = append(env,
		"HOME="+discovery,
		"USERPROFILE="+discovery,
		"APPDATA="+filepath.Join(discovery, "roaming"),
		"LOCALAPPDATA="+filepath.Join(discovery, "local"),
		"XDG_DATA_HOME="+filepath.Join(discovery, "data"),
		"XDG_CONFIG_HOME="+filepath.Join(discovery, "config"),
		"UC_PROFILE=",
	)
	cmd.Env = env
	cmd.Dir = w.root
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("profile_init_failed")
	}
	return nil
}
func (w *worker) submitContent(ctx context.Context, v submitRequest) (submitResult, error) {
	f, ok := w.fixtures[v.Fixture]
	if !ok {
		return submitResult{}, fmt.Errorf("unknown_fixture")
	}
	out := submitResult{Entry: "production_cli"}
	switch f.Meta.Kind {
	case "html":
		s := clip.Snapshot{}
		for i, p := range f.Meta.Parts {
			data, err := os.ReadFile(f.Paths[i])
			if err != nil {
				return out, fmt.Errorf("fixture_read_failed")
			}
			s.Parts = append(s.Parts, clip.Part{Format: p.MIME, Data: data})
		}
		out.Entry = "local_api_html_injector"
		out.StartedNS = time.Now().UnixNano()
		result, err := w.client.Submit(ctx, api.SubmitInput{RequestID: v.Key, Snapshot: s})
		out.FinishedNS = time.Now().UnixNano()
		out.Result = result
		return out, err
	default:
		args := []string{"send", "--to", "all", "--json", "--request-id", v.Key}
		var input *os.File
		switch f.Meta.Kind {
		case "text":
			var err error
			input, err = os.Open(f.Paths[0])
			if err != nil {
				return out, fmt.Errorf("fixture_read_failed")
			}
			defer func() { _ = input.Close() }()
		case "png":
			args = append(args, "--png", f.Paths[0])
		case "file", "directory":
			args = append(args, "-f")
			args = append(args, f.Paths...)
		default:
			return out, fmt.Errorf("invalid_submit_kind")
		}
		// A nil *os.File is not a nil io.Reader. Keep absent stdin untyped.
		var reader io.Reader
		if input != nil {
			reader = input
		}
		out.StartedNS = time.Now().UnixNano()
		result, err := w.cli(ctx, args, reader)
		out.FinishedNS = time.Now().UnixNano()
		if err != nil {
			return out, err
		}
		var report struct {
			Submission engine.SubmissionResult `json:"submission"`
			Send       struct {
				Operation *engine.Operation `json:"operation"`
				Outcome   string            `json:"outcome"`
			} `json:"send"`
		}
		if err = json.Unmarshal(result.Out, &report); err != nil {
			return out, fmt.Errorf("invalid_submission_result")
		}
		out.Result = report.Submission
		out.InitialCLIResultKnown = true
		out.InitialCLIExit = result.Code
		out.InitialSendOutcome = report.Send.Outcome
		if f.Meta.Kind == "directory" && result.Code == 4 && report.Send.Outcome == "timeout" && out.Result.OperationID != "" {
			// A persistent service continues after the CLI's 60 s observation window.
			// Observe this same operation within the predeclared 180 s case budget; never resubmit.
			settled, cancel := context.WithDeadline(ctx, time.Unix(0, out.StartedNS).Add(180*time.Second))
			defer cancel()
			for {
				op, e := w.client.Operation(settled, string(out.Result.OperationID))
				if e == nil {
					out.LastOperation = &op
				}
				if e != nil {
					return out, fmt.Errorf("timed_out_directory_operation_unavailable")
				}
				if op.Done() {
					report.Send.Operation = &op
					if op.Outcome != engine.OutcomeSuccess {
						return out, fmt.Errorf("timed_out_directory_operation_%s", op.Outcome)
					}
					out.SettledAfterTimeout = true
					out.FinishedNS = time.Now().UnixNano()
					break
				}
				select {
				case <-settled.Done():
					return out, fmt.Errorf("directory_operation_exceeded_180s_budget")
				case <-time.After(100 * time.Millisecond):
				}
			}
		}
		if op := report.Send.Operation; op != nil {
			out.PreparationAdmissionMS = float64(op.CreatedAtMs) - float64(out.StartedNS)/1e6
			out.SendOperationMS = float64(op.UpdatedAtMs - op.CreatedAtMs)
		}
		if result.Code != 0 && !out.SettledAfterTimeout {
			var codes []string
			if report.Send.Operation != nil {
				for _, target := range report.Send.Operation.Targets {
					if target.Error != nil {
						codes = append(codes, string(target.Error.Code))
					}
				}
			}
			return out, fmt.Errorf("submit_failed_exit_%d_outcome_%s_codes_%s", result.Code, report.Send.Outcome, strings.Join(codes, ","))
		}
		return out, nil
	}
}
func (w *worker) received(ctx context.Context) ([]engine.ReceivedContent, error) {
	b, err := w.checkedCLI(ctx, "recv", "list")
	if err != nil {
		return nil, err
	}
	var out []engine.ReceivedContent
	if err = json.Unmarshal(b, &out); err != nil {
		return nil, fmt.Errorf("invalid_received_result")
	}
	return out, nil
}
func matches(r engine.ReceivedContent, g armRequest) bool {
	if string(r.PeerID) != g.Source {
		return false
	}
	if g.Fixture.Kind == "directory" {
		for _, p := range r.Content.Parts {
			if p.MIME == "application/vnd.unic.directory+json" {
				return true
			}
		}
		return false
	}
	if len(r.Content.Parts) != len(g.Fixture.Parts) {
		return false
	}
	for i, p := range r.Content.Parts {
		e := g.Fixture.Parts[i]
		if string(p.Kind) != e.Kind || p.MIME != e.MIME || p.Size != e.Size || (e.Kind == "file" && p.Name != e.Name) {
			return false
		}
	}
	return true
}
func (w *worker) newReceipts(ctx context.Context) ([]engine.ReceivedContent, error) {
	if w.gate == nil {
		return nil, fmt.Errorf("receive_not_armed")
	}
	rs, err := w.received(ctx)
	if err != nil {
		return nil, err
	}
	var out []engine.ReceivedContent
	for _, r := range rs {
		if !w.known[string(r.ReceiptID)] && matches(r, *w.gate) {
			out = append(out, r)
		}
	}
	return out, nil
}
func (w *worker) waitContent(ctx context.Context) (delivery, error) {
	for {
		if ctx.Err() != nil {
			return delivery{}, fmt.Errorf("receive_timeout")
		}
		rs, err := w.newReceipts(ctx)
		if err != nil {
			if ctx.Err() != nil {
				return delivery{}, fmt.Errorf("receive_timeout")
			}
			return delivery{}, err
		}
		if len(rs) > 1 {
			return delivery{}, fmt.Errorf("duplicate_delivery")
		}
		if len(rs) == 1 {
			r := rs[0]
			if r.HistoryID == "" {
				return delivery{}, fmt.Errorf("missing_received_history")
			}
			dir, err := os.MkdirTemp(w.root, "export-")
			if err != nil {
				return delivery{}, fmt.Errorf("export_directory_failed")
			}
			defer func() { _ = os.RemoveAll(dir) }()
			if _, err = w.checkedCLI(ctx, "history", "export", string(r.HistoryID), "--output", dir); err != nil {
				if ctx.Err() != nil {
					return delivery{}, fmt.Errorf("verification_timeout")
				}
				return delivery{}, err
			}
			if w.gate.Fixture.Kind == "directory" {
				return w.verifyDirectory(ctx, r, dir)
			}
			files, err := os.ReadDir(dir)
			if err != nil || len(files) != len(w.gate.Fixture.Parts) {
				return delivery{}, fmt.Errorf("export_part_count_mismatch")
			}
			actualParts := []expectedPart{}
			for i, p := range w.gate.Fixture.Parts {
				name := p.Name
				if p.Kind != "file" {
					extension := map[string]string{"text/plain": "txt", "text/html": "html", "image/png": "png"}[p.MIME]
					name = fmt.Sprintf("part-%d.%s", i+1, extension)
				}
				sha, size, err := fileSHA(filepath.Join(dir, name))
				if err != nil || sha != p.SHA || size != p.Size {
					return delivery{}, fmt.Errorf("export_hash_mismatch")
				}
				actualParts = append(actualParts, expectedPart{Kind: p.Kind, MIME: p.MIME, Name: name, Size: size, SHA: sha})
			}
			return delivery{Actual: actualParts, Receipt: string(r.ReceiptID), History: string(r.HistoryID), ReceivedMS: r.ReceivedAtMs, VerifiedNS: time.Now().UnixNano(), Count: 1}, nil
		}
		select {
		case <-ctx.Done():
			return delivery{}, fmt.Errorf("receive_timeout")
		case <-time.After(50 * time.Millisecond):
		}
	}
}
func (w *worker) handle(ctx context.Context, r request) (any, error) {
	limit := 4 * time.Minute
	if r.Method == "wait" {
		v, err := decodeValue[waitRequest](r.Value)
		if err != nil || v.TimeoutSeconds < 1 || v.TimeoutSeconds > 900 {
			return nil, fmt.Errorf("invalid_receive_timeout")
		}
		limit = time.Duration(v.TimeoutSeconds) * time.Second
	}
	ctx, cancel := context.WithTimeout(ctx, limit)
	defer cancel()
	switch r.Method {
	case "directory":
		v, err := decodeValue[directoryCommand](r.Value)
		if err != nil {
			return nil, fmt.Errorf("invalid_request")
		}
		return w.directoryCommand(ctx, v)
	case "hello":
		id, free, err := machineInfo(w.root)
		if err != nil {
			return nil, fmt.Errorf("machine_probe_failed")
		}
		self, err := os.Executable()
		if err != nil {
			return nil, fmt.Errorf("worker_path_failed")
		}
		sha, _, err := fileSHA(self)
		if err != nil {
			return nil, fmt.Errorf("worker_hash_failed")
		}
		return hello{Socket: w.socket, Protocol: protocolVersion, OS: runtime.GOOS, Arch: runtime.GOARCH, Machine: digest([]byte(w.runID + id)), WorkerSHA: sha, Root: w.root, FreeBytes: free}, nil
	case "clock":
		return time.Now().UnixNano(), nil
	case "configure":
		v, err := decodeValue[installConfig](r.Value)
		if err != nil {
			return nil, fmt.Errorf("invalid_request")
		}
		return true, w.configure(ctx, v)
	case "start":
		return true, w.start(ctx)
	case "stop":
		return true, w.stop()
	case "state":
		return w.client.Status(ctx)
	case "history":
		var items []api.HistoryItem
		cursor := ""
		for {
			page, err := w.client.QueryHistory(ctx, api.HistoryQuery{Limit: 200, Cursor: cursor})
			if err != nil {
				return nil, fmt.Errorf("history_query_failed")
			}
			items = append(items, page.Items...)
			if page.NextCursor == "" {
				break
			}
			cursor = page.NextCursor
		}
		sort.Slice(items, func(i, j int) bool { return items[i].ID < items[j].ID })
		return historySnapshot{Items: items}, nil
	case "prepare":
		v, err := decodeValue[prepareRequest](r.Value)
		if err != nil {
			return nil, fmt.Errorf("invalid_request")
		}
		return w.prepare(v)
	case "release":
		id, err := decodeValue[string](r.Value)
		if err != nil {
			return nil, fmt.Errorf("invalid_request")
		}
		f, ok := w.fixtures[id]
		if !ok {
			return nil, fmt.Errorf("unknown_fixture")
		}
		if len(f.Paths) > 0 {
			if err = os.RemoveAll(filepath.Join(w.root, "fixtures", id)); err != nil {
				return nil, fmt.Errorf("fixture_cleanup_failed")
			}
		}
		delete(w.fixtures, id)
		return true, nil
	case "retry":
		v, err := decodeValue[submitRequest](r.Value)
		if err != nil {
			return nil, fmt.Errorf("invalid_request")
		}
		results := make([]engine.SubmissionResult, 4)
		errs := make([]error, 4)
		var wg sync.WaitGroup
		for i := range results {
			wg.Add(1)
			go func(i int) { defer wg.Done(); out, err := w.submit(ctx, v); results[i] = out.Result; errs[i] = err }(i)
		}
		wg.Wait()
		for _, err := range errs {
			if err != nil {
				return nil, err
			}
		}
		return results, nil
	case "submit":
		v, err := decodeValue[submitRequest](r.Value)
		if err != nil {
			return nil, fmt.Errorf("invalid_request")
		}
		return w.submit(ctx, v)
	case "rearm":
		v, err := decodeValue[armRequest](r.Value)
		if err != nil || w.gate == nil || w.gate.Source != v.Source {
			return nil, fmt.Errorf("invalid_receive_rearm")
		}
		w.gate = &v
		return true, nil
	case "arm":
		v, err := decodeValue[armRequest](r.Value)
		if err != nil {
			return nil, fmt.Errorf("invalid_request")
		}
		rs, err := w.received(ctx)
		if err != nil {
			return nil, err
		}
		w.gate = &v
		w.known = map[string]bool{}
		for _, r := range rs {
			w.known[string(r.ReceiptID)] = true
		}
		return true, nil
	case "wait":
		return w.wait(ctx)
	case "audit":
		rs, err := w.newReceipts(ctx)
		return len(rs), err
	case "cli":
		v, err := decodeValue[cliRequest](r.Value)
		if err != nil || len(v.Args) == 0 {
			return nil, fmt.Errorf("invalid_request")
		}
		out, err := w.cli(ctx, v.Args, nil)
		if err != nil {
			return nil, err
		}
		if out.Code != 0 && !v.AllowFailure {
			return nil, fmt.Errorf("cli_%s_failed", v.Args[0])
		}
		return out, nil
	case "policy":
		v, err := decodeValue[map[string]bool](r.Value)
		if err != nil {
			return nil, fmt.Errorf("invalid_request")
		}
		if enabled, ok := v["enabled"]; ok {
			_, err = w.checkedCLI(ctx, "config", "set", "sync.enabled", fmt.Sprint(enabled))
			return true, err
		}
		text, ok := v["text"]
		if !ok {
			return nil, fmt.Errorf("invalid_policy")
		}
		cfg := fmt.Sprintf("device_name = %q\n[telemetry]\nenabled=false\n[plugins]\nlua=[\"example.lua-sensitive\",\"example.lua-emailtag\"]\n[plugins.instances.\"builtin.security\"]\nenabled=false\n[plugins.instances.\"example.lua-emailtag\".config]\ntag=%q\n[plugins.instances.\"builtin.tailcat\".sync]\ntext=%t\n", "lab-"+w.name, "local-"+w.name, text)
		path := filepath.Join(w.root, "policy.toml")
		if err = os.WriteFile(path, []byte(cfg), 0600); err != nil {
			return nil, fmt.Errorf("policy_write_failed")
		}
		_, err = w.checkedCLI(ctx, "config", "apply", path)
		return true, err
	case "observe_auto":
		v, err := decodeValue[observeAutoRequest](r.Value)
		if err != nil || v.Fixture == "" {
			return nil, fmt.Errorf("invalid_request")
		}
		return w.observeAuto(ctx, v)
	case "send_with_progress":
		v, err := decodeValue[sendProgressRequest](r.Value)
		if err != nil || v.Fixture == "" || v.Key == "" {
			return nil, fmt.Errorf("invalid_request")
		}
		return w.sendWithProgress(ctx, v)
	case "channel_policy":
		v, err := decodeValue[channelPolicyRequest](r.Value)
		if err != nil {
			return nil, fmt.Errorf("invalid_request")
		}
		cfg := fmt.Sprintf("device_name = %q\n[telemetry]\nenabled=false\n[plugins]\nlua=[\"example.lua-sensitive\",\"example.lua-emailtag\"]\n[plugins.instances.\"builtin.security\"]\nenabled=false\n[plugins.instances.\"example.lua-emailtag\".config]\ntag=%q\n[plugins.instances.\"builtin.tailcat\".sync]\ntext=%t\nimages=%t\nfiles=%t\n",
			"lab-"+w.name, "local-"+w.name, v.Text, v.Images, v.Files)
		path := filepath.Join(w.root, "channel-policy.toml")
		if err = os.WriteFile(path, []byte(cfg), 0600); err != nil {
			return nil, fmt.Errorf("policy_write_failed")
		}
		_, err = w.checkedCLI(ctx, "config", "apply", path)
		return true, err
	case "notify_network":
		nctx, cancel := context.WithTimeout(ctx, 10*time.Second)
		defer cancel()
		if err := w.client.NotifyNetworkChanged(nctx); err != nil {
			return nil, fmt.Errorf("notify_network_failed")
		}
		return true, nil
	case "refresh":
		deviceID, _ := decodeValue[string](r.Value)
		rctx, cancel := context.WithTimeout(ctx, 15*time.Second)
		defer cancel()
		st, err := w.client.RefreshDevices(rctx, deviceID)
		if err != nil {
			return nil, fmt.Errorf("refresh_devices_failed")
		}
		return st, nil
	case "close":
		keep, err := decodeValue[bool](r.Value)
		if err != nil {
			return nil, fmt.Errorf("invalid_request")
		}
		return w.close(keep)
	default:
		return nil, fmt.Errorf("unknown_worker_method")
	}
}

func (w *worker) closeAdapter() {
	if w.adapterSession == "" {
		return
	}
	cctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	_ = w.client.AdapterCall(cctx, w.adapterSession, "close", nil, nil)
	cancel()
	w.adapterSession = ""
	w.adapterEpoch = ""
	w.adapterSeq = 0
}

func (w *worker) ensureAdapter(ctx context.Context) error {
	if w.adapterSession != "" {
		if err := w.client.AdapterCall(ctx, w.adapterSession, "heartbeat", nil, nil); err == nil {
			return nil
		}
		w.closeAdapter()
	}
	var sess string
	if err := w.client.AdapterCall(ctx, "", "register", api.AdapterRegistration{}, &sess); err != nil {
		return fmt.Errorf("adapter_register_failed")
	}
	caps := engine.HostCapabilities{
		ReadFormats:  []string{engine.MimeText, engine.MimeHTML, engine.MimePNG},
		WriteFormats: []string{engine.MimeText, engine.MimeHTML, engine.MimePNG},
		ReadFiles:    true,
		WriteFiles:   true,
		Observe:      engine.AbilityAvailable,
		Apply:        engine.AbilityAvailable,
	}
	if err := w.client.AdapterCall(ctx, sess, "capabilities", caps, nil); err != nil {
		_ = w.client.AdapterCall(ctx, sess, "close", nil, nil)
		return fmt.Errorf("adapter_capabilities_failed")
	}
	epoch := "e2e-" + sess
	if err := w.client.AdapterCall(ctx, sess, "observe", engine.Observation{Epoch: epoch, Sequence: 1, Mode: engine.ObserveBaseline}, nil); err != nil {
		_ = w.client.AdapterCall(ctx, sess, "close", nil, nil)
		return fmt.Errorf("adapter_baseline_failed")
	}
	w.adapterSession = sess
	w.adapterEpoch = epoch
	w.adapterSeq = 1
	return nil
}

func (w *worker) observeAuto(ctx context.Context, v observeAutoRequest) (observeAutoResult, error) {
	f, ok := w.fixtures[v.Fixture]
	if !ok {
		return observeAutoResult{}, fmt.Errorf("unknown_fixture")
	}
	if err := w.ensureAdapter(ctx); err != nil {
		return observeAutoResult{}, err
	}
	var snap clip.Snapshot
	for i, p := range f.Meta.Parts {
		if p.Kind == "file" {
			snap.Files = append(snap.Files, clip.File{Path: f.Paths[i], Name: p.Name, Size: p.Size})
		} else {
			data, err := os.ReadFile(f.Paths[i])
			if err != nil {
				return observeAutoResult{}, fmt.Errorf("fixture_read_failed")
			}
			snap.Parts = append(snap.Parts, clip.Part{Format: p.MIME, Data: data})
		}
	}
	t0 := time.Now()
	var handle engine.ContentHandle
	if err := w.client.AdapterCall(ctx, w.adapterSession, "import", api.AdapterImport{Snapshot: snap}, &handle); err != nil {
		return observeAutoResult{}, fmt.Errorf("adapter_import_failed")
	}
	defer func() {
		rctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		_ = w.client.AdapterCall(rctx, w.adapterSession, "release", handle.LeaseID, nil)
		cancel()
	}()
	w.adapterSeq++
	var obs engine.ObservationResult
	if err := w.client.AdapterCall(ctx, w.adapterSession, "observe", engine.Observation{
		Epoch:    w.adapterEpoch,
		Sequence: w.adapterSeq,
		Mode:     engine.ObserveChange,
		LeaseID:  handle.LeaseID,
	}, &obs); err != nil {
		return observeAutoResult{}, fmt.Errorf("adapter_observe_failed")
	}
	out := observeAutoResult{
		Disposition: obs.Disposition,
		OperationID: obs.OperationID,
	}
	if obs.Disposition == engine.DispSubmitted && obs.OperationID != "" {
		deadline := time.Now().Add(60 * time.Second)
		for time.Now().Before(deadline) {
			op, err := w.client.Operation(ctx, string(obs.OperationID))
			if err == nil && op.Done() {
				out.Outcome = op.Outcome
				break
			}
			select {
			case <-ctx.Done():
				return out, ctx.Err()
			case <-time.After(100 * time.Millisecond):
			}
		}
	}
	out.ElapsedMS = float64(time.Since(t0)) / float64(time.Millisecond)
	return out, nil
}

func (w *worker) sendWithProgress(ctx context.Context, v sendProgressRequest) (sendProgressResult, error) {
	f, ok := w.fixtures[v.Fixture]
	if !ok {
		return sendProgressResult{}, fmt.Errorf("unknown_fixture")
	}
	var snap clip.Snapshot
	for i, p := range f.Meta.Parts {
		if p.Kind == "file" {
			snap.Files = append(snap.Files, clip.File{Path: f.Paths[i], Name: p.Name, Size: p.Size})
		} else {
			data, err := os.ReadFile(f.Paths[i])
			if err != nil {
				return sendProgressResult{}, fmt.Errorf("fixture_read_failed")
			}
			snap.Parts = append(snap.Parts, clip.Part{Format: p.MIME, Data: data})
		}
	}

	evCtx, cancelEv := context.WithCancel(ctx)
	var opEventsMu sync.Mutex
	var opEvents int
	evDone := make(chan struct{})
	go func() {
		defer close(evDone)
		_ = w.client.Events(evCtx, func(ev api.Event) {
			if ev.Kind == "operations" {
				opEventsMu.Lock()
				opEvents++
				opEventsMu.Unlock()
			}
		})
	}()
	defer func() {
		cancelEv()
		<-evDone
	}()
	// Give SSE subscription a moment to establish before submission starts.
	select {
	case <-ctx.Done():
		return sendProgressResult{}, ctx.Err()
	case <-time.After(100 * time.Millisecond):
	}

	t0 := time.Now()
	sub, err := w.client.Submit(ctx, api.SubmitInput{RequestID: v.Key, Snapshot: snap})
	if err != nil {
		return sendProgressResult{}, fmt.Errorf("submit_failed: %w", err)
	}
	out := sendProgressResult{
		HistoryID:   sub.HistoryID,
		OperationID: sub.OperationID,
		Monotonic:   true,
	}
	if sub.OperationID == "" {
		out.ElapsedMS = float64(time.Since(t0)) / float64(time.Millisecond)
		return out, fmt.Errorf("no_operation_created: disp=%s", sub.SyncDisposition)
	}

	timeout := time.Duration(v.TimeoutSeconds) * time.Second
	if timeout <= 0 {
		timeout = 120 * time.Second
	}
	deadline := time.Now().Add(timeout)
	var lastBytes int64 = -1
	var lastSample *progressSample
	recordSample := func(op engine.Operation) {
		if op.HistoryID != "" {
			out.OperationHistoryID = op.HistoryID
		}
		if len(op.Targets) == 0 {
			return
		}
		tgt := op.Targets[0]
		s := progressSample{
			State:        tgt.State,
			ReceiptStage: tgt.ReceiptStage,
		}
		if tgt.Progress != nil {
			s.BytesSent = tgt.Progress.BytesSent
			s.TotalBytes = tgt.Progress.TotalBytes
			s.PartIndex = tgt.Progress.PartIndex
			s.PartCount = tgt.Progress.PartCount
			if lastBytes >= 0 && s.BytesSent < lastBytes {
				out.Monotonic = false
			}
			lastBytes = s.BytesSent
			if s.TotalBytes > 0 && s.BytesSent > 0 && s.BytesSent < s.TotalBytes {
				out.SawIntermediate = true
			}
		}
		if tgt.State == engine.TargetSending && tgt.ReceiptStage == engine.ReceiptStageLocalWritten {
			out.SawAwaitingReceipt = true
		}
		if lastSample == nil || *lastSample != s {
			cp := s
			lastSample = &cp
			out.Samples = append(out.Samples, s)
		}
	}

	var finalOp engine.Operation
	for time.Now().Before(deadline) {
		if ops, err := w.client.Operations(ctx, api.OperationsFilter{HistoryID: string(sub.HistoryID)}); err == nil {
			for _, op := range ops {
				if op.ID == sub.OperationID {
					out.FoundByHistoryQuery = true
					recordSample(op)
					finalOp = op
					break
				}
			}
		}
		if !out.FoundByActiveQuery {
			if ops, err := w.client.Operations(ctx, api.OperationsFilter{Active: true}); err == nil {
				for _, op := range ops {
					if op.ID == sub.OperationID {
						out.FoundByActiveQuery = true
						break
					}
				}
			}
		}
		if finalOp.ID == "" {
			if op, err := w.client.Operation(ctx, string(sub.OperationID)); err == nil {
				recordSample(op)
				finalOp = op
			}
		}
		if finalOp.ID != "" && finalOp.Done() {
			break
		}
		select {
		case <-ctx.Done():
			return out, ctx.Err()
		case <-time.After(15 * time.Millisecond):
		}
	}

	// Allow final SSE event flush.
	select {
	case <-ctx.Done():
	case <-time.After(100 * time.Millisecond):
	}
	opEventsMu.Lock()
	out.OperationEvents = opEvents
	opEventsMu.Unlock()

	out.ElapsedMS = float64(time.Since(t0)) / float64(time.Millisecond)
	out.Outcome = finalOp.Outcome
	if len(finalOp.Targets) > 0 {
		tgt := finalOp.Targets[0]
		out.FinalState = tgt.State
		out.FinalReceiptStage = tgt.ReceiptStage
		if tgt.Progress != nil {
			out.FinalBytesSent = tgt.Progress.BytesSent
			out.FinalTotalBytes = tgt.Progress.TotalBytes
		}
		if tgt.Error != nil {
			out.FinalErrorCode = tgt.Error.Code
		}
	}
	if !finalOp.Done() {
		return out, fmt.Errorf("operation_timeout")
	}
	return out, nil
}
