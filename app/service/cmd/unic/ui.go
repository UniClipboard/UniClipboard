package main

import (
	"fmt"
	"io"
	"os"
	"sync"
	"time"

	"github.com/charmbracelet/lipgloss"
	"golang.org/x/term"
)

// Terminal output rules: styling and animation appear only when the stream is a
// terminal. Pipes, files, NO_COLOR and TERM=dumb get plain text, so agents and
// scripts parse the same bytes on every run. Stdout stays data; progress goes to stderr.

func interactive(f *os.File) bool {
	return term.IsTerminal(int(f.Fd())) && os.Getenv("NO_COLOR") == "" && os.Getenv("TERM") != "dumb"
}

var (
	errTTY = interactive(os.Stderr)
	outTTY = interactive(os.Stdout)
)

func style(tty bool, s lipgloss.Style, text string) string {
	if !tty {
		return text
	}
	return s.Render(text)
}

var (
	sBold = lipgloss.NewStyle().Bold(true)
	sDim  = lipgloss.NewStyle().Faint(true)
	sCmd  = lipgloss.NewStyle().Foreground(lipgloss.Color("6"))
	sErr  = lipgloss.NewStyle().Foreground(lipgloss.Color("1")).Bold(true)
	sOK   = lipgloss.NewStyle().Foreground(lipgloss.Color("2"))
)

// errorLine is the one-line failure format on stderr.
func errorLine(msg string) string { return style(errTTY, sErr, "error:") + " " + msg }

// note prints a short hint on stderr.
func note(format string, a ...any) {
	fmt.Fprintln(os.Stderr, style(errTTY, sDim, fmt.Sprintf(format, a...)))
}

// spinner animates one status line on stderr. Without a terminal it prints the
// message once and stays silent afterwards.
type spinner struct {
	w    io.Writer
	stop chan struct{}
	done sync.WaitGroup
	once sync.Once
}

var spinFrames = []string{"⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"}

func startSpinner(msg string) *spinner {
	s := &spinner{w: os.Stderr, stop: make(chan struct{})}
	if !errTTY {
		fmt.Fprintln(s.w, msg)
		return s
	}
	s.done.Add(1)
	go func() {
		defer s.done.Done()
		t := time.NewTicker(90 * time.Millisecond)
		defer t.Stop()
		for i := 0; ; i++ {
			fmt.Fprintf(s.w, "\r\x1b[2K%s %s", style(true, sCmd, spinFrames[i%len(spinFrames)]), msg)
			select {
			case <-s.stop:
				fmt.Fprint(s.w, "\r\x1b[2K")
				return
			case <-t.C:
			}
		}
	}()
	return s
}

// Stop clears the animation. It is safe to call more than once.
func (s *spinner) Stop() {
	s.once.Do(func() {
		close(s.stop)
		s.done.Wait()
	})
}

// Done clears the animation and prints a final line.
func (s *spinner) Done(msg string) {
	s.Stop()
	fmt.Fprintln(s.w, style(errTTY, sOK, "✓")+" "+msg)
}
