package ui

import (
	"fmt"
	"os"
	"strings"
	"sync"
	"time"
)

var (
	spinnerTicks = []string{"◒", "◐", "◓"}
	active       *Spinner
)

// Spinner is an indicatif-style steady-tick spinner on stderr. Like
// indicatif, it draws nothing when stderr is not a terminal.
type Spinner struct {
	mu       sync.Mutex
	message  string
	tick     int
	done     chan struct{}
	hidden   bool
	total    uint64
	position uint64
	bytes    bool
}

// NewSpinner starts a spinner rendering ` {spinner}  {msg}`.
func NewSpinner(message string) *Spinner { return start(&Spinner{message: message}) }

// NewByteProgress starts a byte progress bar:
//
//	◐  Receiving file 4.0 MiB/8.0 MiB (50%) [=====>      ]
func NewByteProgress(total uint64, message string) *Spinner {
	return start(&Spinner{message: message, total: total, bytes: true})
}

func start(s *Spinner) *Spinner {
	s.hidden = !StderrIsTerminal()
	s.done = make(chan struct{})
	if s.hidden {
		return s
	}
	mu.Lock()
	active = s
	s.draw()
	mu.Unlock()
	go func() {
		ticker := time.NewTicker(120 * time.Millisecond)
		defer ticker.Stop()
		for {
			select {
			case <-s.done:
				return
			case <-ticker.C:
				mu.Lock()
				s.mu.Lock()
				s.tick++
				s.mu.Unlock()
				if active == s {
					s.draw()
				}
				mu.Unlock()
			}
		}
	}()
	return s
}

func (s *Spinner) line() string {
	s.mu.Lock()
	defer s.mu.Unlock()
	glyph := spinnerTicks[s.tick%len(spinnerTicks)]
	if !s.bytes {
		return fmt.Sprintf(" %s  %s", glyph, s.message)
	}
	percent := uint64(0)
	if s.total > 0 {
		percent = s.position * 100 / s.total
	}
	head := fmt.Sprintf(" %s  %s %s/%s (%d%%) [", glyph, s.message, HumanBytes(s.position), HumanBytes(s.total), percent)
	width := terminalWidth() - len([]rune(head)) - 1
	if width < 0 {
		width = 0
	}
	filled := 0
	if s.total > 0 {
		filled = int(uint64(width) * s.position / s.total)
	}
	bar := strings.Repeat("=", filled)
	if filled < width {
		bar += ">" + strings.Repeat(" ", width-filled-1)
	}
	return head + bar + "]"
}

func (s *Spinner) draw() { fmt.Fprint(os.Stderr, "\r\x1b[2K"+s.line()) }

// SetMessage replaces the spinner message.
func (s *Spinner) SetMessage(message string) {
	s.mu.Lock()
	s.message = message
	s.mu.Unlock()
}

// SetPosition updates byte progress.
func (s *Spinner) SetPosition(position uint64) {
	s.mu.Lock()
	s.position = position
	s.mu.Unlock()
}

// SetLength updates the byte progress total.
func (s *Spinner) SetLength(total uint64) {
	s.mu.Lock()
	s.total = total
	s.mu.Unlock()
}

// Clear removes the spinner line (indicatif `finish_and_clear`).
func (s *Spinner) Clear() {
	select {
	case <-s.done:
		return
	default:
		close(s.done)
	}
	if s.hidden {
		return
	}
	mu.Lock()
	if active == s {
		active = nil
	}
	fmt.Fprint(os.Stderr, "\r\x1b[2K")
	mu.Unlock()
}

// FinishSuccess clears the spinner and prints a success line.
func (s *Spinner) FinishSuccess(message string) {
	s.Clear()
	Success(message)
}

// FinishError clears the spinner and prints an error line.
func (s *Spinner) FinishError(message string) {
	s.Clear()
	Error(message)
}

func activeSpinnerClear() {
	if active != nil {
		fmt.Fprint(os.Stderr, "\r\x1b[2K")
	}
}

func activeSpinnerRedraw() {
	if active != nil {
		active.draw()
	}
}

// HumanBytes formats binary byte units like indicatif's `BinaryBytes`.
func HumanBytes(n uint64) string {
	units := []string{"B", "KiB", "MiB", "GiB", "TiB", "PiB"}
	if n < 1024 {
		return fmt.Sprintf("%d B", n)
	}
	value := float64(n)
	unit := 0
	for value >= 1024 && unit < len(units)-1 {
		value /= 1024
		unit++
	}
	return fmt.Sprintf("%.2f %s", value, units[unit])
}
