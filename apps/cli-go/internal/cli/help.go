package cli

import (
	"fmt"
	"sort"
	"strings"
)

const longIndent = "          "

func (f *Flag) spec() string {
	var b strings.Builder
	if f.Short != 0 {
		fmt.Fprintf(&b, "-%c, ", f.Short)
	} else {
		b.WriteString("    ")
	}
	b.WriteString("--" + f.Long)
	if f.Kind != Bool {
		b.WriteString(" <" + f.ValueName + ">")
	}
	return b.String()
}

func (f *Flag) display() string {
	if f.Kind == Bool {
		return "--" + f.Long
	}
	return "--" + f.Long + " <" + f.ValueName + ">"
}

func (a *Positional) display() string {
	if a.Required {
		return "<" + a.Name + ">"
	}
	return "[" + a.Name + "]"
}

// sortedFlags returns visible flags in clap display order: (order, long).
func (c *Command) sortedFlags() []*Flag {
	var flags []*Flag
	for _, f := range c.allFlags() {
		if !f.Hidden {
			flags = append(flags, f)
		}
	}
	sort.SliceStable(flags, func(i, j int) bool {
		if flags[i].order != flags[j].order {
			return flags[i].order < flags[j].order
		}
		return flags[i].Long < flags[j].Long
	})
	return flags
}

// usageLines renders the `Usage:` lines without the prefix.
func (c *Command) usageLines() []string {
	head := "uniclip" + strings.TrimPrefix(c.path(), "uniclip")
	parts := []string{head}
	hasOptional := true // -h/--help is always optional
	if hasOptional {
		parts = append(parts, "[OPTIONS]")
	}
	for _, f := range c.sortedFlags() {
		if f.Required {
			parts = append(parts, f.display())
		}
	}
	for i := range c.Groups {
		if c.Groups[i].Required {
			parts = append(parts, c.groupDisplay(&c.Groups[i]))
		}
	}
	for _, a := range c.Args {
		parts = append(parts, a.display())
	}
	if len(c.Subs) > 0 && c.ArgsConflictSubs {
		return []string{strings.Join(parts, " "), head + " <COMMAND>"}
	}
	if len(c.Subs) > 0 {
		if c.SubRequired {
			parts = append(parts, "<COMMAND>")
		} else {
			parts = append(parts, "[COMMAND]")
		}
	}
	return []string{strings.Join(parts, " ")}
}

func usageBlock(lines []string) string {
	return "Usage: " + strings.Join(lines, "\n       ")
}

type helpRow struct {
	left, help, long, defaultValue string
	possible                       []string
}

// Help renders `-h` (long=false) or `--help` (long=true) text.
func (c *Command) Help(long bool) string {
	useLong := long && c.hasLongContent()
	var b strings.Builder
	about := c.About
	if long && c.LongAbout != "" {
		about = c.LongAbout
	}
	if about != "" {
		b.WriteString(about + "\n\n")
	}
	b.WriteString(usageBlock(c.usageLines()) + "\n")

	if subs := c.visibleSubs(); len(subs) > 0 {
		rows := make([]helpRow, 0, len(subs)+1)
		for _, s := range subs {
			rows = append(rows, helpRow{left: s.Name, help: s.About})
		}
		rows = append(rows, helpRow{left: "help", help: "Print this message or the help of the given subcommand(s)"})
		b.WriteString("\nCommands:\n")
		writeRows(&b, rows, false)
	}
	if len(c.Args) > 0 {
		rows := make([]helpRow, 0, len(c.Args))
		for _, a := range c.Args {
			rows = append(rows, helpRow{left: a.display(), help: a.Help, long: a.LongHelp})
		}
		b.WriteString("\nArguments:\n")
		writeRows(&b, rows, useLong)
	}
	rows := []helpRow{}
	for _, f := range c.sortedFlags() {
		row := helpRow{left: f.spec(), help: f.Help, long: f.LongHelp, defaultValue: f.Default}
		if f.Kind == Enum {
			row.possible = f.PossibleValue
		}
		rows = append(rows, row)
	}
	helpText := "Print help"
	if c.hasLongContent() {
		if useLong {
			helpText = "Print help (see a summary with '-h')"
		} else {
			helpText = "Print help (see more with '--help')"
		}
	}
	rows = append(rows, helpRow{left: "-h, --help", help: helpText})
	if c.parent == nil {
		rows = append(rows, helpRow{left: "-V, --version", help: "Print version"})
	}
	b.WriteString("\nOptions:\n")
	writeRows(&b, rows, useLong)
	return b.String()
}

func writeRows(b *strings.Builder, rows []helpRow, next bool) {
	if next {
		for i, r := range rows {
			text := r.help
			if r.long != "" {
				text = r.long
			}
			lines := strings.Split(text, "\n")
			if r.defaultValue != "" {
				lines = append(lines, "", "[default: "+r.defaultValue+"]")
			}
			if len(r.possible) > 0 {
				lines = append(lines, "", "[possible values: "+strings.Join(r.possible, ", ")+"]")
			}
			b.WriteString("  " + r.left + "\n")
			for _, line := range lines {
				b.WriteString(longIndent + line + "\n")
			}
			if i < len(rows)-1 {
				b.WriteString("\n")
			}
		}
		return
	}
	width := 0
	for _, r := range rows {
		if n := len([]rune(r.left)); n > width {
			width = n
		}
	}
	for _, r := range rows {
		text := r.help
		if r.defaultValue != "" {
			text += " [default: " + r.defaultValue + "]"
		}
		if len(r.possible) > 0 {
			text += " [possible values: " + strings.Join(r.possible, ", ") + "]"
		}
		pad := strings.Repeat(" ", width-len([]rune(r.left)))
		b.WriteString("  " + r.left + pad + "  " + text + "\n")
	}
}
