// Package cli declares the uniclip command tree and adapts it to cobra.
//
// The command specs carry the user-visible help text and argument rules;
// cobra/pflag tokenize and dispatch. help.go renders help and errors in the
// clap v4 layout the Rust CLI established, so terminal output and exit codes
// stay compatible for scripts.
package cli

// FlagKind selects how a flag value is parsed.
type FlagKind int

const (
	Bool    FlagKind = iota
	String           // single string value
	Strings          // repeatable string value
	Uint             // unsigned integer of Bits width (default 64)
	Int              // signed 64-bit integer
	Enum             // one of PossibleValues
	Range            // unsigned integer within [Min, Max]
)

// Flag is one option. Display order is its position in Command.Flags
// (globals use their position in the root), with ties broken by long name,
// matching clap.
type Flag struct {
	Long          string
	Short         rune
	ValueName     string // empty for Bool
	Help          string // `-h` text
	LongHelp      string // `--help` text; defaults to Help
	Default       string
	PossibleValue []string
	Kind          FlagKind
	Min, Max      uint64
	Bits          int
	Required      bool
	Hidden        bool
	Global        bool
	ConflictsWith []string // other flag longs or positional names
	Requires      []string
	order         int
}

// Group is a set of mutually exclusive flags.
type Group struct {
	Members  []string
	Required bool
}

func (c *Command) groupOf(long string) *Group {
	for i := range c.Groups {
		for _, m := range c.Groups[i].Members {
			if m == long {
				return &c.Groups[i]
			}
		}
	}
	return nil
}

func (c *Command) groupDisplay(g *Group) string {
	parts := make([]string, 0, len(g.Members))
	for _, m := range g.Members {
		parts = append(parts, c.findFlag(m).display())
	}
	return "<" + joinPipe(parts) + ">"
}

func joinPipe(parts []string) string {
	out := ""
	for i, p := range parts {
		if i > 0 {
			out += "|"
		}
		out += p
	}
	return out
}

// Positional is one positional argument.
type Positional struct {
	Name          string // value name, e.g. TEXT_OR_FILE
	Help          string
	LongHelp      string
	Required      bool
	ConflictsWith []string
}

// Command is one node of the command tree.
type Command struct {
	Name      string
	Aliases   []string
	Hidden    bool
	About     string // short help (used in command lists and `-h`)
	LongAbout string // `--help` about; defaults to About
	Flags     []*Flag
	Args      []*Positional
	Subs      []*Command
	// SubRequired renders `<COMMAND>` and shows help on stderr (exit 2)
	// when no subcommand is given.
	SubRequired bool
	// ArgsConflictSubs renders two usage lines (args, or a subcommand).
	ArgsConflictSubs bool
	// Groups are clap ArgGroups: exactly one member when Required.
	Groups []Group
	// Before runs (root only) after parsing, before any command.
	Before func(*Context)
	Run    func(*Context) int
	parent *Command
}

func (c *Command) path() string {
	if c.parent == nil {
		return c.Name
	}
	return c.parent.path() + " " + c.Name
}

func (c *Command) root() *Command {
	if c.parent == nil {
		return c
	}
	return c.parent.root()
}

// visibleFlags lists the command's own flags plus inherited globals in clap
// display order.
func (c *Command) allFlags() []*Flag {
	flags := append([]*Flag{}, c.Flags...)
	if c.parent != nil {
		for _, g := range c.root().Flags {
			if g.Global {
				flags = append(flags, g)
			}
		}
	}
	return flags
}

func (c *Command) hasLongContent() bool {
	if c.LongAbout != "" && c.LongAbout != c.About {
		return true
	}
	for _, f := range c.allFlags() {
		if !f.Hidden && f.LongHelp != "" && f.LongHelp != f.Help {
			return true
		}
	}
	for _, a := range c.Args {
		if a.LongHelp != "" && a.LongHelp != a.Help {
			return true
		}
	}
	return false
}

func (c *Command) findFlag(long string) *Flag {
	for _, f := range c.allFlags() {
		if f.Long == long {
			return f
		}
	}
	return nil
}

func (c *Command) visibleSubs() []*Command {
	var subs []*Command
	for _, s := range c.Subs {
		if !s.Hidden {
			subs = append(subs, s)
		}
	}
	return subs
}

func link(c *Command) {
	for i, f := range c.Flags {
		f.order = i
	}
	for _, s := range c.Subs {
		s.parent = c
		link(s)
	}
}
