// Package releaseinfo exposes immutable build identity without starting a host.
package releaseinfo

import (
	"encoding/json"
	"fmt"
	"io"
)

// The packager sets these values from release/products.toml using -ldflags.
var Version = "dev"
var BuildNumber = "0"
var Source = "unknown"

// Print handles version requests before configuration, telemetry or GUI startup.
func Print(out io.Writer, program, product string, args []string) bool {
	if len(args) == 0 || (args[0] != "version" && args[0] != "--version") {
		return false
	}
	if len(args) == 2 && args[1] == "--json" {
		_ = json.NewEncoder(out).Encode(map[string]string{"program": program, "product": product, "version": Version, "build_number": BuildNumber, "source_sha": Source})
	} else {
		_, _ = fmt.Fprintf(out, "%s %s (%s)\n", program, Version, BuildNumber)
	}
	return true
}
