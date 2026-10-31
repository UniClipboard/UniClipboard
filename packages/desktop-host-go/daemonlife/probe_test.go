package daemonlife

import (
	"errors"
	"testing"
)

func TestClassifyStartupStatus(t *testing.T) {
	for _, state := range []string{"preparing", "upgrading", "starting_services"} {
		active, err := classifyStartupStatus([]byte(`{"progress":{"state":"`+state+`"}}`), "test")
		if err != nil || !active {
			t.Fatalf("state %s: active=%t err=%v", state, active, err)
		}
	}
	active, err := classifyStartupStatus([]byte(`{"progress":{"state":"ready"}}`), "test")
	if err != nil || active {
		t.Fatalf("ready state: active=%t err=%v", active, err)
	}
	active, err = classifyStartupStatus([]byte(`{"progress":{"state":"failed","failure":{"reason":"backup_failed","retryable":true}}}`), "test")
	var startupErr *Error
	if active || !errors.As(err, &startupErr) || startupErr.Kind != ErrStartupFailed || !startupErr.Retryable {
		t.Fatalf("failed state: active=%t err=%v", active, err)
	}
}
