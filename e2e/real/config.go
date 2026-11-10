package main

import (
	"fmt"
	"regexp"
	"sort"
	"strings"

	"github.com/BurntSushi/toml"
)

type lab struct {
	Version int                   `toml:"version"`
	Nodes   map[string]nodeConfig `toml:"nodes"`
	Suite   suiteConfig           `toml:"suite"`
}
type nodeConfig struct {
	Transport string `toml:"transport"`
	Host      string `toml:"host"`
}
type suiteConfig struct {
	ReceiveTimeoutSeconds int      `toml:"receive_timeout_seconds" json:"receive_timeout_seconds"`
	Topology              string   `toml:"topology" json:"topology"`
	Repetitions           int      `toml:"repetitions" json:"repetitions"`
	Scenarios             []string `toml:"scenarios" json:"scenarios"`
	TextBytes             []int64  `toml:"text_bytes" json:"text_bytes"`
	PNGPixels             []int    `toml:"png_pixels" json:"png_pixels"`
	FileBytes             []int64  `toml:"file_bytes" json:"file_bytes"`
	HTMLBytes             []int64  `toml:"html_bytes" json:"html_bytes"`
}

var nodeLabel = regexp.MustCompile(`^[A-Za-z][A-Za-z0-9_-]{0,31}$`)

func readLab(path string) (lab, error) {
	c := lab{Suite: suiteConfig{ReceiveTimeoutSeconds: 180, Topology: "mesh", Repetitions: 3, Scenarios: []string{"contract", "pairwise", "mesh", "isolation"}, TextBytes: []int64{1, 1024, 65536, 1048576}, PNGPixels: []int{16, 256, 1024}, FileBytes: []int64{0, 1024, 1048576, 16777216, 104857600}, HTMLBytes: []int64{1024, 65536, 1048576}}}
	md, err := toml.DecodeFile(path, &c)
	if err != nil {
		return c, fmt.Errorf("cannot decode lab configuration")
	}
	if len(md.Undecoded()) != 0 {
		return c, fmt.Errorf("unknown lab configuration field: %s", md.Undecoded()[0])
	}
	if c.Version != 1 || len(c.Nodes) == 0 {
		return c, fmt.Errorf("version=1 and at least one node are required")
	}
	for name, n := range c.Nodes {
		if !nodeLabel.MatchString(name) {
			return c, fmt.Errorf("invalid node label")
		}
		switch n.Transport {
		case "local":
			if n.Host != "" {
				return c, fmt.Errorf("local node %s cannot have a host", name)
			}
		case "ssh":
			if n.Host == "" || strings.HasPrefix(n.Host, "-") || strings.ContainsAny(n.Host, " \t\r\n\x00") {
				return c, fmt.Errorf("invalid SSH host for %s", name)
			}
		default:
			return c, fmt.Errorf("node %s requires transport=local or ssh", name)
		}
	}
	s := c.Suite
	if s.ReceiveTimeoutSeconds < 1 || s.ReceiveTimeoutSeconds > 900 {
		return c, fmt.Errorf("receive_timeout_seconds must be between 1 and 900")
	}
	if s.Topology != "mesh" || s.Repetitions < 1 || s.Repetitions > 1000 {
		return c, fmt.Errorf("suite requires topology=mesh and repetitions between 1 and 1000")
	}
	seen := map[string]bool{}
	for _, v := range s.Scenarios {
		if seen[v] {
			return c, fmt.Errorf("duplicate scenario")
		}
		seen[v] = true
		switch v {
		case "contract", "pairwise", "mesh", "isolation":
		default:
			return c, fmt.Errorf("unknown scenario: %s", v)
		}
	}
	if len(s.Scenarios) == 0 {
		return c, fmt.Errorf("at least one scenario is required")
	}
	for _, n := range s.TextBytes {
		if n < 1 || n > 64<<20 {
			return c, fmt.Errorf("text_bytes must be between 1 and 64 MiB")
		}
	}
	for _, n := range s.HTMLBytes {
		if n < 1 || n > 16<<20 {
			return c, fmt.Errorf("html_bytes must be between 1 and 16 MiB")
		}
	}
	for _, n := range s.FileBytes {
		if n < 0 || n > 100<<20 {
			return c, fmt.Errorf("file_bytes must be between 0 and 100 MiB")
		}
	}
	for _, n := range s.PNGPixels {
		if n < 1 || n > 2048 {
			return c, fmt.Errorf("png_pixels must be between 1 and 2048")
		}
	}
	if len(payloads(s)) == 0 {
		return c, fmt.Errorf("at least one payload size is required")
	}
	return c, nil
}
func labels(c lab) []string {
	names := make([]string, 0, len(c.Nodes))
	for name := range c.Nodes {
		names = append(names, name)
	}
	sort.Strings(names)
	return names
}

type payload struct {
	Kind string `json:"kind"`
	Size int64  `json:"size"`
}

func payloads(c suiteConfig) []payload {
	var out []payload
	for _, n := range c.TextBytes {
		out = append(out, payload{"text", n})
	}
	for _, n := range c.PNGPixels {
		out = append(out, payload{"png", int64(n)})
	}
	for _, n := range c.FileBytes {
		out = append(out, payload{"file", n})
	}
	for _, n := range c.HTMLBytes {
		out = append(out, payload{"html", n})
	}
	return out
}

func plannedMeasurements(c lab) int {
	n := len(c.Nodes)
	enabled := map[string]bool{}
	for _, v := range c.Suite.Scenarios {
		enabled[v] = true
	}
	phases := 0
	if enabled["pairwise"] && !(n == 2 && enabled["mesh"]) {
		phases++
	}
	if enabled["mesh"] {
		phases++
	}
	return phases * n * (n - 1) * len(payloads(c.Suite)) * c.Suite.Repetitions
}
func plannedPayloadBound(c lab) int64 {
	var bytes int64
	for _, p := range payloads(c.Suite) {
		size := p.Size
		switch p.Kind {
		case "png":
			size = 4*size*size + (1 << 20)
		case "html":
			size = 2*size + 44
		}
		bytes += size
	}
	if len(payloads(c.Suite)) == 0 {
		return 0
	}
	return int64(plannedMeasurements(c)/len(payloads(c.Suite))) * bytes
}
