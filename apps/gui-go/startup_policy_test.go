package main

import "testing"

func TestLaunchVisibilitySeparatesAutomaticAndExplicitActivation(t *testing.T) {
	for _, background := range []bool{false, true} {
		if !shouldOpenAtLaunch(background, false, false, false) {
			t.Fatalf("explicit launch hidden with background preference %t", background)
		}
		if got := shouldOpenAtLaunch(background, true, false, false); got == background {
			t.Fatalf("automatic launch visibility %t with background preference %t", got, background)
		}
		if shouldOpenAtLaunch(background, false, true, false) {
			t.Fatal("windowless shell restart opened a window")
		}
		if !shouldOpenAtLaunch(background, true, true, true) {
			t.Fatal("isolated acceptance override was ignored")
		}
	}
}

func TestRepeatedLaunchClassification(t *testing.T) {
	for _, tc := range []struct {
		args []string
		want secondLaunchAction
	}{
		{nil, actionShowMainWindow},
		{[]string{"--autostart"}, actionIgnoreAutostart},
		{[]string{"--quick-panel"}, actionToggleQuickPanel},
		{[]string{"--autostart", "--quick-panel"}, actionToggleQuickPanel},
	} {
		if got := classifySecondLaunch(tc.args); got != tc.want {
			t.Fatalf("%v: got %s, want %s", tc.args, got, tc.want)
		}
	}
}
