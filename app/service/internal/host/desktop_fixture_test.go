package host

import (
	"context"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clipboardintegration"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/internal/observability"
	"github.com/uniclipboard/unic/internal/session"
)

type desktopOptions struct {
	Dir           string
	Settings      *config.Manager
	Board         clipboardintegration.Board
	Poll          time.Duration
	Logf          func(string, ...any)
	Transport     session.Transport
	Plugins       []engine.PluginInstance
	LuaDir        string
	Observability observability.Options
}

func openDesktop(ctx context.Context, o desktopOptions) (*Node, error) {
	n, err := Open(ctx, Options{Dir: o.Dir, Settings: o.Settings, Logf: o.Logf, Transport: o.Transport, Plugins: o.Plugins, LuaDir: o.LuaDir, Observability: o.Observability})
	if err != nil {
		return nil, err
	}
	_ = n.e.UpdateHostCapabilities(ctx, engine.HostCapabilities{ReadFormats: []string{engine.MimeText, engine.MimeHTML, engine.MimePNG}, WriteFormats: []string{engine.MimeText, engine.MimeHTML, engine.MimePNG}, ReadFiles: true, WriteFiles: true, Observe: engine.AbilityAvailable, Apply: engine.AbilityAvailable})
	c := clipboardintegration.New(clipboardintegration.Local{Engine: n.e}, o.Board, o.Poll, n.hub.emit)
	n.clip = c
	return n, nil
}
