package main

import (
	"bytes"
	"strings"
	"testing"
)

func TestExtractGlobalFlags(t *testing.T) {
	cases := []struct {
		name        string
		args        []string
		wantCmd     []string
		wantProfile string
		wantJSON    bool
		wantErr     bool
	}{
		{
			name:        "profile before command",
			args:        []string{"--profile", "work", "init", "--fresh"},
			wantCmd:     []string{"init", "--fresh"},
			wantProfile: "work",
		},
		{
			name:        "profile after command",
			args:        []string{"init", "--profile", "work", "--name", "Mac"},
			wantCmd:     []string{"init", "--name", "Mac"},
			wantProfile: "work",
		},
		{
			name:        "short profile equals and leading json",
			args:        []string{"--json", "-p=lab1", "status"},
			wantCmd:     []string{"status"},
			wantProfile: "lab1",
			wantJSON:    true,
		},
		{
			name:    "stops at double dash",
			args:    []string{"send", "--", "-p", "literal"},
			wantCmd: []string{"send", "--", "-p", "literal"},
		},
		{
			name:    "missing profile value",
			args:    []string{"--profile"},
			wantErr: true,
		},
		{
			name:    "empty profile value",
			args:    []string{"--profile="},
			wantErr: true,
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			cmd, profile, js, err := extractGlobalFlags(tc.args)
			if tc.wantErr {
				if err == nil {
					t.Fatalf("expected error, got cmd=%v profile=%q", cmd, profile)
				}
				return
			}
			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
			if strings.Join(cmd, "|") != strings.Join(tc.wantCmd, "|") || profile != tc.wantProfile || js != tc.wantJSON {
				t.Fatalf("got cmd=%v profile=%q json=%v; want cmd=%v profile=%q json=%v", cmd, profile, js, tc.wantCmd, tc.wantProfile, tc.wantJSON)
			}
		})
	}
}

func TestClassifyNewCommandsAndHelp(t *testing.T) {
	for _, tc := range []struct {
		cmd  string
		args []string
		leaf string
		req  requirement
	}{
		{"init", nil, "init", reqOffline},
		{"daemon", []string{"start"}, "daemon start", reqLocal},
		{"daemon", []string{"stop"}, "daemon stop", reqLocal},
		{"daemon", []string{"restart"}, "daemon restart", reqLocal},
		{"daemon", []string{"status"}, "daemon status", reqLocal},
		{"daemon", []string{"logs", "-n", "10"}, "daemon logs", reqLocal},
		{"status", nil, "status", reqLocal},
		{"profile", nil, "profile", reqLocal},
		{"profile", []string{"list"}, "profile list", reqLocal},
		{"profile", []string{"path"}, "profile path", reqLocal},
		{"doctor", nil, "doctor", reqLocal},
		{"completion", []string{"zsh"}, "completion", reqLocal},
		{"device", []string{"rename", "abc", "Laptop"}, "device rename", reqService},
		{"device", []string{"suggest", "list"}, "device suggest", reqService},
		{"channel", []string{"add", "inst", "grp"}, "channel add", reqService},
		{"channel", []string{"pause", "id", "1"}, "channel pause", reqService},
		// Per-command --help always classifies as reqLocal without touching unicd.
		{"send", []string{"--help"}, "send", reqLocal},
		{"device", []string{"--help"}, "device", reqLocal},
		{"device", []string{"add", "--help"}, "device add", reqLocal},
		{"daemon", []string{"start", "-h"}, "daemon start", reqLocal},
	} {
		l, _, err := classify(tc.cmd, tc.args)
		if err != nil {
			t.Fatalf("classify(%q, %v) failed: %v", tc.cmd, tc.args, err)
		}
		if l.Name != tc.leaf || l.Requirement != tc.req {
			t.Fatalf("classify(%q, %v) = (%q, %q), want (%q, %q)", tc.cmd, tc.args, l.Name, l.Requirement, tc.leaf, tc.req)
		}
	}
}

func TestCompletionScripts(t *testing.T) {
	for _, sh := range []string{"bash", "zsh", "fish"} {
		var buf bytes.Buffer
		if err := runCompletion([]string{sh}, &buf); err != nil {
			t.Fatalf("completion %s failed: %v", sh, err)
		}
		if !strings.Contains(buf.String(), "daemon") || !strings.Contains(buf.String(), "profile") {
			t.Fatalf("completion %s missing expected commands", sh)
		}
	}
	if err := runCompletion([]string{"tcsh"}, &bytes.Buffer{}); err == nil {
		t.Fatal("unsupported shell should fail")
	}
}
