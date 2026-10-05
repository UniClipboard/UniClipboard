package ui

import (
	"errors"
	"fmt"
	"os"
	"strings"
	"unicode/utf8"

	"golang.org/x/term"
)

// errNotTerminal matches dialoguer's error text for non-interactive stderr.
var errNotTerminal = errors.New("IO error: not a terminal")

const maskChar = "•"

// keyReader reads raw keys from the controlling terminal.
type keyReader struct {
	in    *os.File
	state *term.State
	owned bool
}

func openKeys() (*keyReader, error) {
	in := os.Stdin
	owned := false
	if !term.IsTerminal(int(in.Fd())) {
		tty, err := openTTY()
		if err != nil {
			return nil, errNotTerminal
		}
		in, owned = tty, true
	}
	state, err := term.MakeRaw(int(in.Fd()))
	if err != nil {
		if owned {
			in.Close()
		}
		return nil, fmt.Errorf("IO error: %v", err)
	}
	return &keyReader{in: in, state: state, owned: owned}, nil
}

func (k *keyReader) close() {
	term.Restore(int(k.in.Fd()), k.state)
	if k.owned {
		k.in.Close()
	}
}

type key struct {
	ch    rune
	enter bool
	back  bool
	esc   bool
	intr  bool
}

func (k *keyReader) read() (key, error) {
	buf := make([]byte, 8)
	n, err := k.in.Read(buf[:1])
	if err != nil || n == 0 {
		return key{}, fmt.Errorf("IO error: %v", err)
	}
	switch b := buf[0]; {
	case b == '\r' || b == '\n':
		return key{enter: true}, nil
	case b == 0x7f || b == 0x08:
		return key{back: true}, nil
	case b == 0x03:
		return key{intr: true}, nil
	case b == 0x1b:
		return key{esc: true}, nil
	case b < 0x80:
		return key{ch: rune(b)}, nil
	default:
		size := 1
		switch {
		case b&0xE0 == 0xC0:
			size = 2
		case b&0xF0 == 0xE0:
			size = 3
		case b&0xF8 == 0xF0:
			size = 4
		}
		for i := 1; i < size; i++ {
			if _, err := k.in.Read(buf[i : i+1]); err != nil {
				break
			}
		}
		r, _ := utf8.DecodeRune(buf[:size])
		return key{ch: r}, nil
	}
}

func writeErr(s string) { fmt.Fprint(os.Stderr, s) }

// errReadInterrupted is console's io::Error for Ctrl-C read in raw mode.
var errReadInterrupted = errors.New("read interrupted")

// interrupt mirrors console's handling of Ctrl-C in raw mode: restore the
// terminal, raise SIGINT at this process (which ends it unless SIGINT is
// ignored or handled), then report the interrupted read to the caller.
func interrupt(k *keyReader) error {
	k.close()
	raiseInterrupt()
	return errReadInterrupted
}

// Confirm renders ` ?  Prompt [y/N]` and resolves on y/n/Enter to
// ` ✓  Prompt yes`, like dialoguer's Confirm with the Uniclip theme.
func Confirm(prompt string, def bool) (bool, error) {
	if !StderrIsTerminal() {
		return false, errNotTerminal
	}
	suffix := "[y/N]"
	if def {
		suffix = "[Y/n]"
	}
	writeErr(fmt.Sprintf(" %s  %s %s", Yellow("?"), prompt, Dim(suffix)))
	keys, err := openKeys()
	if err != nil {
		writeErr("\n")
		return false, err
	}
	writeErr("\x1b[?25l")
	var value bool
	for {
		k, err := keys.read()
		if err != nil {
			keys.close()
			return false, err
		}
		if k.intr {
			return false, fmt.Errorf("IO error: %w", interrupt(keys))
		}
		if k.ch == 'y' || k.ch == 'Y' {
			value = true
			break
		}
		if k.ch == 'n' || k.ch == 'N' {
			value = false
			break
		}
		if k.enter {
			value = def
			break
		}
	}
	keys.close()
	answer := "no"
	if value {
		answer = "yes"
	}
	writeErr(fmt.Sprintf("\r\x1b[2K %s  %s %s\n\x1b[?25h", Green("✓"), prompt, Dim(answer)))
	return value, nil
}

// Input reads a single line. allowEmpty=false re-prompts until non-empty.
func Input(prompt string, allowEmpty bool) (string, error) {
	if !StderrIsTerminal() {
		return "", errNotTerminal
	}
	for {
		writeErr(fmt.Sprintf(" %s  %s ", Yellow("?"), prompt))
		keys, err := openKeys()
		if err != nil {
			writeErr("\n")
			return "", err
		}
		var line []rune
		for {
			k, err := keys.read()
			if err != nil {
				keys.close()
				return "", err
			}
			if k.intr {
				return "", fmt.Errorf("IO error: %w", interrupt(keys))
			}
			if k.enter {
				break
			}
			if k.back {
				if len(line) > 0 {
					line = line[:len(line)-1]
					writeErr("\b \b")
				}
				continue
			}
			if k.ch != 0 && !k.esc {
				line = append(line, k.ch)
				writeErr(string(k.ch))
			}
		}
		keys.close()
		value := string(line)
		if value == "" && !allowEmpty {
			writeErr("\r\x1b[2K")
			continue
		}
		writeErr(fmt.Sprintf("\r\x1b[2K %s  %s %s\n", Green("✓"), prompt, Dim(value)))
		return value, nil
	}
}

// Password reads a masked password:
//
//	?  Prompt label
//	│  ••••
//
// collapsing to ` ✓  Prompt label ••••••••` after Enter.
func Password(prompt string) (string, error) {
	writeErr(fmt.Sprintf(" %s  %s\n", Yellow("?"), prompt))
	writeErr(fmt.Sprintf(" %s  ", Dim("│")))
	keys, err := openKeys()
	if err != nil {
		return "", fmt.Errorf("password input failed: %v", err)
	}
	var input []rune
	for {
		k, err := keys.read()
		if err != nil {
			keys.close()
			return "", fmt.Errorf("password input failed: %v", err)
		}
		switch {
		case k.intr:
			return "", fmt.Errorf("password input failed: %w", interrupt(keys))
		case k.enter:
			keys.close()
			writeErr("\r\x1b[2K\x1b[1A\r\x1b[2K")
			writeErr(fmt.Sprintf(" %s  %s %s\n", Green("✓"), prompt, Dim(strings.Repeat(maskChar, len(string(input))))))
			return string(input), nil
		case k.back:
			if len(input) > 0 {
				input = input[:len(input)-1]
				writeErr("\b \b")
			}
		case k.esc:
			keys.close()
			writeErr("\r\x1b[2K\x1b[1A\r\x1b[2K")
			return "", errors.New("password input cancelled")
		case k.ch != 0:
			input = append(input, k.ch)
			writeErr(maskChar)
		}
	}
}

// PasswordWithConfirm asks twice until both entries match.
func PasswordWithConfirm(prompt, confirmPrompt string) (string, error) {
	for {
		first, err := Password(prompt)
		if err != nil {
			return "", err
		}
		second, err := Password(confirmPrompt)
		if err != nil {
			return "", err
		}
		if first == second {
			return first, nil
		}
		Error("Passphrases do not match, try again")
	}
}
