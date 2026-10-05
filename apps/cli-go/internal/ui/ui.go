// Package ui renders every human-readable terminal line of the CLI on stderr
// with the shared visual template:
//
//	{glyph}  {content}
//
// (one leading space, one glyph, two spaces). Colors follow the Rust
// `console` crate rules so output is byte-identical when stderr is not a TTY.
package ui

import (
	"fmt"
	"os"
	"strings"
	"sync"

	"golang.org/x/term"
)

const (
	ansiReset  = "\x1b[0m"
	ansiBold   = "1"
	ansiDim    = "2"
	ansiRed    = "31"
	ansiGreen  = "32"
	ansiYellow = "33"
	ansiCyan   = "36"
)

// StderrIsTerminal reports whether stderr is attached to a terminal.
func StderrIsTerminal() bool { return term.IsTerminal(int(os.Stderr.Fd())) }

// colorsEnabled mirrors console's `default_colors_enabled` for stderr.
var colorsEnabled = sync.OnceValue(func() bool {
	supported := StderrIsTerminal() && !envSet("NO_COLOR") && os.Getenv("TERM") != "dumb"
	clicolor := os.Getenv("CLICOLOR")
	force := os.Getenv("CLICOLOR_FORCE")
	return (supported && (clicolor == "" || clicolor != "0")) || (force != "" && force != "0")
})

func envSet(name string) bool {
	_, ok := os.LookupEnv(name)
	return ok
}

func style(text string, codes ...string) string {
	if !colorsEnabled() || len(codes) == 0 {
		return text
	}
	return "\x1b[" + strings.Join(codes, ";") + "m" + text + ansiReset
}

// Styled exposes the palette to other renderers (help, prompts).
func Cyan(s string) string   { return style(s, ansiCyan) }
func Green(s string) string  { return style(s, ansiGreen) }
func Yellow(s string) string { return style(s, ansiYellow) }
func Red(s string) string    { return style(s, ansiRed) }
func Dim(s string) string    { return style(s, ansiDim) }
func Bold(s string) string   { return style(s, ansiBold) }

var mu sync.Mutex

func writeLine(line string) {
	mu.Lock()
	defer mu.Unlock()
	activeSpinnerClear()
	fmt.Fprintln(os.Stderr, line)
	activeSpinnerRedraw()
}

// Header prints `◆  Title` preceded by a blank line.
func Header(text string) {
	writeLine(fmt.Sprintf("\n %s  %s", style("◆", ansiCyan, ansiBold), Bold(text)))
}

// Success prints `✓  Message`.
func Success(text string) { writeLine(fmt.Sprintf(" %s  %s", Green("✓"), text)) }

// Warn prints `⚠  Message`.
func Warn(text string) { writeLine(fmt.Sprintf(" %s  %s", Yellow("⚠"), text)) }

// Error prints `✗  Message`.
func Error(text string) { writeLine(fmt.Sprintf(" %s  %s", Red("✗"), text)) }

// Info prints `│  label: value` with a dim prefix.
func Info(label, value string) {
	writeLine(fmt.Sprintf(" %s  %s %s", Dim("│"), Dim(label+":"), value))
}

// Bar prints a dim separator ` │`.
func Bar() { writeLine(fmt.Sprintf(" %s", Dim("│"))) }

// End prints a closing corner `└  Message`.
func End(text string) { writeLine(" └  " + text) }

// VerificationCode prints `│  Verification code: CODE` with the code highlighted.
func VerificationCode(code string) {
	writeLine(fmt.Sprintf(" %s  Verification code: %s", Dim("│"), style(code, ansiCyan, ansiBold)))
}

// RawStderr writes an unstyled line to stderr (the Rust `eprintln!` paths).
func RawStderr(text string) { writeLine(text) }

// StyleError and StyleTip color clap-format argument errors.
func StyleError(s string) string { return style(s, ansiRed, ansiBold) }
func StyleTip(s string) string   { return style(s, ansiGreen) }
