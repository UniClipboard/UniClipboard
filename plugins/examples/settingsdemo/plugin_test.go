package settingsdemo

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/uniclipboard/unic/plugin"
)

func TestInitUsesSchemaAndBusinessValidation(t *testing.T) {
	for _, tc := range []struct {
		name string
		cfg  plugin.Config
		ok   bool
	}{
		{"defaults", nil, true},
		{"integer exponent", plugin.Config{"max_length": json.Number("1e3")}, true},
		{"explicit disabled", plugin.Config{"enabled": false}, true},
		{"unknown", plugin.Config{"private-key": "private-value"}, false},
		{"invalid choice", plugin.Config{"mode": "html"}, false},
		{"invalid limit", plugin.Config{"max_length": 0}, false},
		{"invalid tag", plugin.Config{"tags": []string{""}}, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			err := (&Plugin{}).Init(context.Background(), nil, tc.cfg)
			if (err == nil) != tc.ok {
				t.Fatalf("Init success=%v, want %v", err == nil, tc.ok)
			}
			if err != nil && err != plugin.ErrInvalidConfig {
				t.Fatal("returned a free-text configuration error")
			}
		})
	}
}
