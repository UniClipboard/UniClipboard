package commands

import (
	"fmt"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/userservice"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonlife"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonproc"
)

func serviceTree() *cli.Command {
	return &cli.Command{Name: "service", About: "Manage the user service (Linux systemd / macOS launchd)", SubRequired: true, Subs: []*cli.Command{
		{Name: "start", About: "Install if needed and start the user service at login", Flags: []*cli.Flag{{Long: "server", Kind: cli.Bool, Help: "Run headless without the system clipboard"}}, Run: func(ctx *cli.Context) int { return runService(ctx, "start") }},
		{Name: "restart", About: "Restart the installed user service", Run: func(ctx *cli.Context) int { return runService(ctx, "restart") }},
		{Name: "status", About: "Show service registration, running state and HTTP health", Run: func(ctx *cli.Context) int { return runService(ctx, "status") }},
		{Name: "stop", About: "Stop the user service and disable its login startup", Run: func(ctx *cli.Context) int { return runService(ctx, "stop") }},
	}}
}

type serviceOutput struct {
	userservice.State
	HTTPHealth    string `json:"http_health"`
	HealthDetails string `json:"health_details,omitempty"`
}

func serviceHealth(state userservice.State) serviceOutput {
	out := serviceOutput{State: state, HTTPHealth: "unreachable"}
	health, err := daemonlife.Probe()
	if err != nil {
		out.HTTPHealth = "error"
		out.HealthDetails = err.Error()
		return out
	}
	if health.Kind == daemonlife.Absent {
		return out
	}
	conn, err := daemonproc.ReadConnFile()
	if err != nil {
		out.HTTPHealth = "error"
		out.HealthDetails = err.Error()
		return out
	}
	if !state.Running || conn == nil || int(conn.PID) != state.PID {
		out.HTTPHealth = "other_daemon"
		out.HealthDetails = "profile HTTP endpoint does not belong to the running service"
		return out
	}
	if health.Kind == daemonlife.Incompatible {
		out.HTTPHealth = "incompatible"
		out.HealthDetails = health.Details
		return out
	}
	out.HTTPHealth = health.Health.Status
	return out
}

func runService(ctx *cli.Context, action string) int {
	service, err := userservice.New()
	if err != nil {
		ui.RawStderr("Error: " + err.Error())
		return exitcode.Error
	}
	state, err := service.Status()
	if err == nil {
		switch action {
		case "start":
			if !state.Running {
				err = requireAbsent()
			}
			if err == nil {
				err = service.Start(ctx.Bool("server"))
			}
		case "restart":
			// Do not take over an incumbent outside this service, even if it is unhealthy.
			meta, e := daemonproc.ReadPidMetadata()
			err = e
			if err == nil && meta != nil && daemonproc.IsActiveDaemon(meta.PID) && int(meta.PID) != state.PID {
				err = fmt.Errorf("profile is owned by another daemon; refusing service restart")
			}
			if err == nil {
				err = service.Restart()
			}
		case "stop":
			err = service.Stop()
		}
	}
	if err != nil {
		ui.RawStderr("Error: " + err.Error())
		return exitcode.Error
	}
	if action == "start" || action == "restart" {
		// Poll service identity alongside HTTP, so a foreign healthy daemon cannot count as success.
		deadline := time.Now().Add(daemonlife.StartupTimeout)
		for {
			state, err = service.Status()
			if err != nil {
				break
			}
			health := serviceHealth(state)
			if state.Running && (health.HTTPHealth == "ok" || health.HTTPHealth == "recovery_required") {
				break
			}
			if !time.Now().Before(deadline) {
				err = fmt.Errorf("service did not become HTTP healthy; inspect service status and daemon logs")
				break
			}
			time.Sleep(200 * time.Millisecond)
		}
		if err != nil {
			ui.RawStderr("Error: " + err.Error())
			return exitcode.Error
		}
	} else {
		state, err = service.Status()
	}
	if err != nil {
		ui.RawStderr("Error: " + err.Error())
		return exitcode.Error
	}
	out := serviceHealth(state)
	human := fmt.Sprintf("Service %s: installed=%t loaded=%t running=%t; HTTP health=%s", state.Name, state.Installed, state.Loaded, state.Running, out.HTTPHealth)
	if out.HealthDetails != "" {
		human += " (" + out.HealthDetails + ")"
	}
	code := exitcode.Success
	if action == "status" && (!state.Running || (out.HTTPHealth != "ok" && out.HTTPHealth != "recovery_required")) {
		code = exitcode.Error
	}
	return printOrFail(out, human, ctx.JSON(), code)
}
