package userservice

import "testing"

func TestServiceEnvironmentKeepsTheDaemonConsoleOff(t *testing.T) {
	env := serviceEnvironment("/Users/example", "/opt/uniclip/uniclip", true, func(string) string { return "" })

	if got := env["UC_CONSOLE_LOG"]; got != "off" {
		t.Fatalf("UC_CONSOLE_LOG = %q; the service's stdout file is not rotated, so the daemon console layer must be off", got)
	}
	if env["UNICLIPBOARD_ENV"] != "production" || env["HOME"] != "/Users/example" {
		t.Fatalf("base environment lost: %v", env)
	}
}

func TestServiceEnvironmentDoesNotLetTheCallerTurnTheConsoleBackOn(t *testing.T) {
	env := serviceEnvironment("/Users/example", "/opt/uniclip/uniclip", true, func(key string) string {
		if key == "UC_CONSOLE_LOG" {
			return "on"
		}
		return ""
	})

	if env["UC_CONSOLE_LOG"] != "off" {
		t.Fatalf("UC_CONSOLE_LOG = %q", env["UC_CONSOLE_LOG"])
	}
}

func TestServiceEnvironmentPassesDevelopmentAndXDGOverrides(t *testing.T) {
	values := map[string]string{
		"UNICLIPBOARD_ENV": "development",
		"XDG_DATA_HOME":    "/data",
		"UC_PROFILE":       "dev",
	}
	env := serviceEnvironment("/home/u", "/opt/uniclip/uniclip", false, func(key string) string { return values[key] })

	if env["UNICLIPBOARD_ENV"] != "development" || env["XDG_DATA_HOME"] != "/data" || env["UC_PROFILE"] != "dev" {
		t.Fatalf("environment = %v", env)
	}
}

func TestServiceEnvironmentKeepsTheConsoleWhenStdoutIsNotAnUnboundedFile(t *testing.T) {
	// systemd sends stdout to the journal, which bounds it itself.
	env := serviceEnvironment("/home/u", "/opt/uniclip/uniclip", false, func(string) string { return "" })

	if value, present := env["UC_CONSOLE_LOG"]; present {
		t.Fatalf("UC_CONSOLE_LOG = %q; journald-backed services keep the console layer", value)
	}
}
