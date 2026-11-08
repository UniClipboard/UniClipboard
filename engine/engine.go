// Package engine is unic's public SDK: pairing, trust, sessions and sync
// policy behind typed methods, independent of any UI or system clipboard.
//
// A host (desktop daemon, mobile app) creates one Engine per data directory,
// tells it what the platform can do, and drives it through the methods below.
// The engine never touches the system clipboard or a UI; it does not promise
// to keep running in the background.
//
// The behaviour of every method is specified in docs/design/engine-contract.md;
// change that document together with the code.
package engine

import (
	"context"
	"crypto/rand"
	"crypto/subtle"
	"encoding/hex"
	"errors"
	"log/slog"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"sync"
	"time"

	"github.com/uniclipboard/unic/internal/identity"
	"github.com/uniclipboard/unic/internal/rendezvous"
	"github.com/uniclipboard/unic/internal/session"
	"github.com/uniclipboard/unic/internal/store"
	"github.com/uniclipboard/unic/plugin"
)

// Engine is one running node. All methods are safe for concurrent use.
type Engine struct {
	instance InstanceID
	opts     Options
	res      ResourcePolicy
	log      *slog.Logger

	lock      *dirLock
	ident     *identity.Identity // unified device identity (identity protocol v1)
	st        *store.Store
	instLocks []*dirLock  // runtime.lock of each backend instance
	core      PeerBackend // nil: local-only, no peer backend
	trUsed    bool        // core.Start was called, so core.Close is due
	self      DeviceID
	defName   string

	use sync.RWMutex  // held (read) by every call; Close takes it to free the store
	ctl chan struct{} // serializes suspend/resume

	mu         sync.Mutex // guards everything below
	life       Lifecycle
	closing    bool
	closeBegun bool
	stopReq    bool
	runtime    RuntimeState
	rev        uint64
	settings   SettingsState
	host       HostCapabilities
	subs       map[*Subscription]struct{}
	ops        opTable
	code       codeTimer
	tix        *liveTicket // the pairing ticket on offer
	rvDefault  *rendezvous.Client
	rvCache    *rendezvous.Client

	startCancel context.CancelFunc
	startDone   chan struct{} // closed when Start has fully returned
	runCtx      context.Context
	runCancel   context.CancelFunc
	runDone     chan struct{}
	opWG        sync.WaitGroup

	stopped  chan struct{} // closed when teardown is complete
	closedCh chan struct{} // closed when Close is complete

	trDone        chan struct{} // closed when tr.Start returned
	events        <-chan session.Event
	evCancel      func()
	evStop        chan struct{}
	evDone        chan struct{}
	cs            *contentStore
	hist          *history
	backends      []syncBackend
	dedupInflight map[store.DedupKey]struct{} // receives being read right now; guarded by mu
	syncMu        sync.Mutex                  // orders writes of a record's sync summary
	recv          []*receipt                  // the received list, oldest first
	recvReserved  int                         // list slots set aside for receives being stored; guarded by mu
	recvTimer     *time.Timer
	histTimer     *time.Timer // next expiry sweep; guarded by mu
	obsMu         sync.Mutex  // serializes observation and application
	obs           observation
	ticket        *applyTicket
	lastApply     applyDone

	processors  []*pipelineProcessor // private adapters, fixed before Start
	actions     []*pipelineAction    // likewise
	observers   []*pipelineObserver  // likewise
	plugins     []plugin.Plugin      // started in New, closed with the engine
	submissions map[RequestID]*submissionEntry
}

// New validates opts, takes the data directory, opens the database and the
// device identity. It does not touch the network: Start does.
func New(ctx context.Context, opts Options) (*Engine, error) {
	if ctx == nil {
		return nil, newErr(CodeInvalidArgument, "nil context")
	}
	if err := ctx.Err(); err != nil {
		return nil, ctxErr(err)
	}
	if opts.DataDir == "" {
		return nil, newErr(CodeInvalidArgument, "DataDir is required")
	}
	if reflect.DeepEqual(opts.Settings, Settings{}) {
		opts.Settings = DefaultSettings()
	}
	if err := opts.Settings.validate(); err != nil {
		return nil, err
	}
	var backendIDs []PluginID
	if opts.Peer != nil {
		backendIDs = append(backendIDs, BackendTailcat)
	}
	for _, b := range opts.extraBackends {
		backendIDs = append(backendIDs, b.ID())
	}
	pol, perr := normalizePolicies(opts.Settings, backendIDs)
	if perr != nil {
		return nil, perr
	}
	opts.Settings = pol
	res, rerr := opts.Resources.withDefaults()
	if rerr != nil {
		return nil, rerr
	}
	if res.MaxStoredBytes < opts.Settings.Limits.MaxTotalBytes {
		return nil, newErr(CodeInvalidArgument, "MaxStoredBytes must hold limits.max_total_bytes")
	}
	host, herr := normalizeHost(opts.HostCapabilities)
	if herr != nil {
		return nil, herr
	}
	opts.Resources = res
	if opts.InboxDir == "" {
		opts.InboxDir = filepath.Join(opts.DataDir, "inbox")
	}
	if abs, err := filepath.Abs(opts.InboxDir); err == nil {
		opts.InboxDir = abs
	}
	if err := os.MkdirAll(opts.DataDir, 0o700); err != nil {
		return nil, newErr(CodeUnavailable, "cannot create the data directory")
	}
	lock, err := lockDir(filepath.Join(opts.DataDir, "unic.lock"))
	if errors.Is(err, errLocked) {
		return nil, newErr(CodeInUse, "the data directory is used by another instance")
	} else if err != nil {
		return nil, newErr(CodeUnavailable, "cannot lock the data directory")
	}
	fail := func(e *Error) (*Engine, error) { lock.release(); return nil, e }
	cleanContentDirs(opts.InboxDir) // leftovers of a dead instance; we hold the lock

	st, err := store.Open(filepath.Join(opts.DataDir, "unic.db"))
	if err != nil {
		return fail(newErr(CodeUnavailable, "cannot open the database"))
	}
	closeAll := func(e *Error) (*Engine, error) { _ = st.Close(); return fail(e) }

	hopts, herr2 := opts.History.withDefaults(res.MaxStoredBytes)
	if herr2 != nil {
		return closeAll(herr2)
	}
	opts.History = hopts
	hist, err := openHistory(opts.DataDir, st, hopts)
	if err != nil {
		return closeAll(newErr(CodeUnavailable, "cannot prepare the history"))
	}
	hist.sweep(st)                                     // expired records go before anyone can ask for them
	_, _ = st.InterruptPending(time.Now().UnixMilli()) // unfinished sends are not resumed

	ident, err := openDeviceIdentity(opts.DataDir, st)
	if errors.Is(err, errIdentityMismatch) {
		return closeAll(newErr(CodeIdentityMismatch, "the device identity does not match this data directory"))
	} else if err != nil {
		return closeAll(newErr(CodeUnavailable, "cannot open the device identity"))
	}
	defName, err := defaultName(st, ident.DeviceID())
	if err != nil {
		return closeAll(newErr(CodeUnavailable, "cannot read the device identity"))
	}
	e := &Engine{
		instance: InstanceID(randHex(8)), opts: opts, res: res, log: opts.Logger,
		lock: lock, st: st, ident: ident, defName: defName,
		ctl:      make(chan struct{}, 1),
		life:     LifecycleCreated,
		runtime:  RuntimeActive,
		settings: SettingsState{Revision: 1, Value: opts.Settings},
		host:     host,
		subs:     map[*Subscription]struct{}{},
		ops:      newOpTable(),
		stopped:  make(chan struct{}), closedCh: make(chan struct{}),
		cs:            newContentStore(),
		dedupInflight: map[store.DedupKey]struct{}{},
		hist:          hist,
	}
	rv := opts.rendezvous
	if rv == nil {
		rv = rendezvous.NewWithURL("") // the production service; settings.RendezvousURL overrides it, the environment does not
	}
	e.rvDefault = rv
	if opts.Peer != nil {
		idir, ilock, ierr := openInstanceDir(opts.DataDir, tailcatInstance)
		if ierr != nil {
			if errors.Is(ierr, errLocked) {
				return closeAll(newErr(CodeInUse, "the backend instance is used by another process"))
			}
			return closeAll(newErr(CodeUnavailable, "cannot prepare the backend directory"))
		}
		e.instLocks = append(e.instLocks, ilock)
		core, perr := opts.Peer(PeerEnv{Store: st, Dir: idir, Config: session.Config{
			Identity: ident, Name: defName, Host: tailcatHost{e}, Receiver: receiver{e},
			Logf:     func(string, ...any) {}, // lower layers log addresses and names; the engine logs its own redacted lines
			Settings: settingsSource{e},
		}})
		if perr != nil || core == nil {
			ilock.release()
			return closeAll(newErr(CodeUnavailable, "cannot prepare the network"))
		}
		e.core = core
		e.backends = append(e.backends, tailcatBackend{e})
		e.events, e.evCancel = core.Subscribe()
	}
	e.backends = append(e.backends, opts.extraBackends...)
	e.evStop, e.evDone = make(chan struct{}), make(chan struct{})
	go e.pumpCoreEvents()
	if perr := e.loadPlugins(ctx, opts.Plugins); perr != nil {
		_ = e.Close(context.WithoutCancel(ctx))
		return nil, perr
	}
	e.logf(slog.LevelInfo, "engine created")
	return e, nil
}

// settingsSource feeds the current settings to the session.
type settingsSource struct{ e *Engine }

func (s settingsSource) Current() session.Settings {
	s.e.mu.Lock()
	defer s.e.mu.Unlock()
	v := s.e.settings.Value
	return session.Settings{DeviceName: strings.TrimSpace(v.DeviceName), RendezvousURL: strings.TrimRight(v.RendezvousURL, "/"),
		MaxFileBytes: v.Limits.MaxFileBytes, MaxTotalBytes: v.Limits.MaxTotalBytes, SyncEnabled: v.Sync.Enabled}
}

func (e *Engine) logf(level slog.Level, msg string, args ...any) {
	if e.log != nil {
		e.log.Log(context.Background(), level, msg, args...)
	}
}

func randHex(n int) string {
	b := make([]byte, n)
	_, _ = rand.Read(b)
	return hex.EncodeToString(b)
}

// defaultName is chosen once and kept, so that the name other devices see does
// not change when the host name does.
func defaultName(st *store.Store, id string) (string, error) {
	if b, err := st.Meta("device.default_name"); err != nil {
		return "", err
	} else if b != nil {
		return string(b), nil
	}
	name, _ := os.Hostname()
	if name == "" || len(name) > 64 {
		name = "device-" + id[:6]
	}
	return name, st.SetMeta("device.default_name", []byte(name))
}

// ---- gating ----

// enter admits a call unless the engine is closing; the returned function must
// be called when the call ends.
func (e *Engine) enter(ctx context.Context) (func(), *Error) {
	if ctx == nil {
		return nil, newErr(CodeInvalidArgument, "nil context")
	}
	if err := ctx.Err(); err != nil {
		return nil, ctxErr(err)
	}
	e.use.RLock()
	e.mu.Lock()
	closing := e.closing
	e.mu.Unlock()
	if closing {
		e.use.RUnlock()
		return nil, errClosed
	}
	return e.use.RUnlock, nil
}

// needNetwork reports why network work cannot happen now. e.mu must be held.
func (e *Engine) needNetworkLocked() *Error {
	if err := e.needRunningLocked(); err != nil {
		return err
	}
	if e.core == nil {
		return newErr(CodeUnsupported, "no peer-to-peer backend is installed")
	}
	return nil
}

// needRunningLocked is needNetworkLocked without the backend requirement: the
// content paths use it, because a local-only engine still commits clips.
func (e *Engine) needRunningLocked() *Error {
	if e.life != LifecycleRunning {
		return newErr(CodeInvalidState, "the engine is not running")
	}
	if e.runtime == RuntimeSuspended {
		return newErr(CodeSuspended, "the host suspended networking")
	}
	return nil
}

// ---- lifecycle ----

// Start brings the engine up. Success means the local services are running,
// not that any other device is reachable.
func (e *Engine) Start(ctx context.Context) error {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return gerr
	}
	defer release()

	e.mu.Lock()
	if e.life != LifecycleCreated {
		e.mu.Unlock()
		return newErr(CodeInvalidState, "the engine was already started")
	}
	sctx, scancel := context.WithCancel(ctx)
	startDone := make(chan struct{})
	e.life, e.startCancel, e.startDone = LifecycleStarting, scancel, startDone
	e.commitLocked(EventLifecycle)
	active := e.runtime == RuntimeActive
	e.mu.Unlock()
	defer scancel()

	err := e.startCore(sctx, active)
	if err == nil {
		e.mu.Lock()
		if e.stopReq {
			err = newErr(CodeCancelled, "the engine was stopped while starting")
		} else {
			e.life = LifecycleRunning
			e.commitLocked(EventLifecycle)
			e.scheduleHistorySweepLocked()
		}
		e.mu.Unlock()
	}
	close(startDone) // teardown may now proceed
	if err != nil {
		e.beginStop()
		e.logf(slog.LevelWarn, "start failed", "code", string(asError(err).Code))
		// Recovery continues in the background when ctx ran out or the start was
		// cancelled by Stop; otherwise wait (bounded) so the caller sees stopped.
		if asError(err).Code != CodeCancelled && asError(err).Code != CodeTimeout {
			waitCtx, c := context.WithTimeout(ctx, 10*time.Second)
			defer c()
			select {
			case <-e.stopped:
			case <-waitCtx.Done():
			}
		}
		return asError(err)
	}
	e.notifyObservers(plugin.EventAppStarted, "")
	e.logf(slog.LevelInfo, "engine started")
	return nil
}

// startCore starts the transport and, unless suspended, the session layer.
func (e *Engine) startCore(ctx context.Context, active bool) error {
	if e.core == nil { // local-only: nothing to start, but submissions still need a run context
		if !active {
			return nil
		}
		return e.startRun(ctx)
	}
	started := make(chan error, 1)
	trDone := make(chan struct{})
	e.mu.Lock()
	e.trUsed, e.trDone = true, trDone
	e.mu.Unlock()
	go func() { started <- e.core.Start(); close(trDone) }()
	select {
	case err := <-started:
		if err != nil {
			return newErr(CodeUnavailable, "cannot start the network")
		}
	case <-ctx.Done():
		return ctxErr(ctx.Err()) // teardown closes the transport once Start returns
	}
	if !active {
		return nil
	}
	return e.startRun(ctx)
}

// startRun starts the session layer and waits until it is accepting work.
func (e *Engine) startRun(ctx context.Context) error {
	e.mu.Lock()
	prev := e.runDone
	e.mu.Unlock()
	if prev != nil {
		select {
		case <-prev:
		case <-ctx.Done():
			return ctxErr(ctx.Err())
		}
	}
	e.mu.Lock()
	if e.stopReq || e.runtime != RuntimeActive {
		e.mu.Unlock()
		return newErr(CodeInvalidState, "the engine cannot run now")
	}
	rctx, cancel := context.WithCancel(context.Background())
	done, ready := make(chan struct{}), make(chan struct{})
	e.runCtx, e.runCancel, e.runDone = rctx, cancel, done
	e.mu.Unlock()

	if e.core == nil { // local-only: the run context still bounds submissions
		close(done)
		return nil
	}
	go e.runCore(rctx, done, ready)
	select {
	case <-ready:
		return nil
	case <-done:
		cancel()
		return newErr(CodeUnavailable, "cannot start the session layer")
	case <-ctx.Done():
		cancel()
		return ctxErr(ctx.Err())
	}
}

func (e *Engine) runCore(rctx context.Context, done, ready chan struct{}) {
	defer close(done)
	var once sync.Once
	err := e.core.RunReady(rctx, func() { once.Do(func() { close(ready) }) })
	if err != nil && rctx.Err() == nil {
		e.logf(slog.LevelError, "session layer stopped", "reason", "run_failed")
	}
}

// stopRun stops the session layer and waits for it.
func (e *Engine) stopRun(ctx context.Context) error {
	e.mu.Lock()
	cancel, done := e.runCancel, e.runDone
	e.runCancel = nil
	e.mu.Unlock()
	if cancel != nil {
		cancel()
	}
	if done != nil {
		select {
		case <-done:
		case <-ctx.Done():
			return ctxErr(ctx.Err())
		}
	}
	return nil
}

// beginStop commits to stopping. Teardown then runs to completion whatever the
// callers do.
func (e *Engine) beginStop() {
	e.mu.Lock()
	defer e.mu.Unlock()
	switch e.life {
	case LifecycleCreated:
		e.life = LifecycleStopped
		e.stopReq = true
		e.commitLocked(EventLifecycle)
		close(e.stopped)
	case LifecycleStarting, LifecycleRunning:
		e.life = LifecycleStopping
		e.stopReq = true
		e.killTicketLocked()
		e.commitLocked(EventLifecycle)
		go e.teardown()
	}
}

func (e *Engine) teardown() {
	e.mu.Lock()
	if e.startCancel != nil {
		e.startCancel()
	}
	startDone := e.startDone
	e.mu.Unlock()
	if startDone != nil {
		<-startDone // Start may be inside the transport
	}
	_ = e.stopRun(context.Background())
	e.opWG.Wait()
	e.mu.Lock()
	e.code.stop()
	if e.recvTimer != nil {
		e.recvTimer.Stop()
	}
	if e.histTimer != nil {
		e.histTimer.Stop()
	}
	used, trDone := e.trUsed, e.trDone
	e.mu.Unlock()
	if used {
		<-trDone
		_ = e.core.Close()
	}
	e.mu.Lock()
	e.life = LifecycleStopped
	e.commitLocked(EventLifecycle)
	close(e.stopped)
	e.mu.Unlock()
	e.logf(slog.LevelInfo, "engine stopped")
}

// Stop shuts the network down and waits until nothing is running. Once Stop
// has begun it always runs to completion; if ctx ends first the engine stays
// in stopping and a later call can wait again. The database stays open, so
// queries still work. Stop is idempotent.
func (e *Engine) Stop(ctx context.Context) error {
	if ctx == nil {
		return newErr(CodeInvalidArgument, "nil context")
	}
	e.beginStop()
	select {
	case <-e.stopped:
		return nil
	case <-ctx.Done():
		return ctxErr(ctx.Err())
	}
}

// Close stops the engine and releases the database, the data directory lock
// and every subscription. Calls made after Close has begun fail with closed.
// Close is idempotent; if ctx ends first, call it again to keep waiting.
func (e *Engine) Close(ctx context.Context) error {
	if ctx == nil {
		return newErr(CodeInvalidArgument, "nil context")
	}
	e.mu.Lock()
	e.closing = true
	first := !e.closeBegun
	e.closeBegun = true
	e.mu.Unlock()
	if first {
		e.beginStop()
		go e.finalize()
	}
	select {
	case <-e.closedCh:
		return nil
	case <-ctx.Done():
		return ctxErr(ctx.Err())
	}
}

func (e *Engine) finalize() {
	<-e.stopped
	e.notifyObservers(plugin.EventAppShutdown, "") // delivered while observers are drained below
	close(e.evStop)
	if e.evCancel != nil {
		e.evCancel()
	}
	<-e.evDone
	e.use.Lock() // in-flight calls finish before the store goes away
	e.mu.Lock()
	subs := make([]*Subscription, 0, len(e.subs))
	for s := range e.subs {
		subs = append(subs, s)
	}
	e.subs = map[*Subscription]struct{}{}
	e.life = LifecycleClosed
	e.recv = nil // their content goes with dropContent
	if e.recvTimer != nil {
		e.recvTimer.Stop()
	}
	if e.histTimer != nil {
		e.histTimer.Stop()
	}
	e.mu.Unlock()
	for _, s := range subs {
		s.finish()
	}
	e.closePlugins(context.Background())
	e.dropContent()
	_ = os.RemoveAll(e.recvDir())
	_ = e.st.Close()
	for _, l := range e.instLocks {
		l.release()
	}
	e.lock.release()
	e.use.Unlock()
	e.logf(slog.LevelInfo, "engine closed")
	close(e.closedCh)
}

// SetRuntimeState tells the engine whether the host lets it use the network.
// Suspended closes all sessions and fails transfers in flight; active listens
// and reconnects again. It does not grant OS permissions and is unrelated to
// the sync.enabled setting, which keeps connections up.
func (e *Engine) SetRuntimeState(ctx context.Context, state RuntimeState) error {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return gerr
	}
	defer release()
	if state != RuntimeActive && state != RuntimeSuspended {
		return newErr(CodeInvalidArgument, "unknown runtime state")
	}
	select {
	case e.ctl <- struct{}{}:
		defer func() { <-e.ctl }()
	case <-ctx.Done():
		return ctxErr(ctx.Err())
	}
	e.mu.Lock()
	if e.runtime == state {
		e.mu.Unlock()
		return nil
	}
	e.runtime = state
	if state == RuntimeSuspended {
		e.killTicketLocked()
		e.obs = observation{} // content copied while suspended is not sent on resume
	}
	e.commitLocked(EventRuntime, EventCapabilities)
	running := e.life == LifecycleRunning
	e.mu.Unlock()
	if !running {
		return nil // Start applies it
	}
	if state == RuntimeSuspended {
		return asErrorOrNil(e.stopRun(ctx))
	}
	return asErrorOrNil(e.startRun(ctx))
}

func asErrorOrNil(err error) error {
	if err == nil {
		return nil
	}
	return asError(err)
}

// NotifyNetworkChanged tells the engine the host's network changed. It only
// triggers a fresh look: it does not mean the network works, and it never
// resends old content.
func (e *Engine) NotifyNetworkChanged(ctx context.Context) error {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return gerr
	}
	defer release()
	e.mu.Lock()
	err := e.needNetworkLocked()
	e.mu.Unlock()
	if err != nil {
		return err
	}
	e.core.NetworkChanged()
	return nil
}

var errIdentityMismatch = errors.New("device identity does not match the registered key")

// openDeviceIdentity loads the unified device identity (DataDir/identity). The
// public key is registered in the database on first use; afterwards the key
// file must match it. A missing, damaged or different key file is an error:
// a registered identity is never replaced by a freshly generated one.
func openDeviceIdentity(dataDir string, st *store.Store) (*identity.Identity, error) {
	dir := filepath.Join(dataDir, "identity")
	reg, err := st.Meta("identity.public")
	if err != nil {
		return nil, err
	}
	if reg == nil {
		id, _, err := identity.LoadOrCreate(dir)
		if err != nil {
			return nil, err
		}
		return id, st.SetMeta("identity.public", id.Public())
	}
	id, err := identity.Load(dir)
	if err != nil {
		return nil, err
	}
	if subtle.ConstantTimeCompare(id.Public(), reg) != 1 {
		return nil, errIdentityMismatch
	}
	return id, nil
}
