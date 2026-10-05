package cli

import (
	"errors"
	"fmt"
	"os"
	"regexp"
	"strconv"
	"strings"

	"github.com/spf13/cobra"
	"github.com/spf13/pflag"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/buildinfo"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
)

// usageError is a clap-format argument error (exit 2).
type usageError struct {
	msg   string
	tip   string
	usage []string
}

func (e *usageError) Error() string { return e.msg }

func (e *usageError) render() string {
	var b strings.Builder
	b.WriteString(ui.StyleError("error:") + " " + e.msg + "\n")
	if e.tip != "" {
		b.WriteString("\n  " + ui.StyleTip("tip:") + " " + e.tip + "\n")
	}
	if e.usage != nil {
		b.WriteString("\n" + usageBlock(e.usage) + "\n")
	}
	b.WriteString("\nFor more information, try '--help'.\n")
	return b.String()
}

// Context carries the parsed arguments of one invocation.
type Context struct {
	Cmd    *Command
	Args   []string
	values map[string]*flagValue
	used   []string // flag longs in command-line order
	err    *usageError
}

func (c *Context) value(long string) *flagValue {
	if v, ok := c.values[long]; ok {
		return v
	}
	panic("unknown flag " + long)
}

// Bool reports a boolean flag.
func (c *Context) Bool(long string) bool { return c.value(long).set }

// Has reports whether an option was given.
func (c *Context) Has(long string) bool { return c.value(long).set }

// String returns an option value (the default when absent).
func (c *Context) String(long string) string {
	v := c.value(long)
	if !v.set {
		return v.flag.Default
	}
	return v.values[0]
}

// Strings returns all values of a repeatable option.
func (c *Context) Strings(long string) []string { return c.value(long).values }

// Uint returns an unsigned option value (the default when absent).
func (c *Context) Uint(long string) uint64 {
	n, _ := strconv.ParseUint(c.String(long), 10, 64)
	return n
}

// Arg returns the i-th positional argument and whether it was given.
func (c *Context) Arg(i int) (string, bool) {
	if i < len(c.Args) {
		return c.Args[i], true
	}
	return "", false
}

// JSON and Verbose are the global switches.
func (c *Context) JSON() bool    { return c.Bool("json") }
func (c *Context) Verbose() bool { return c.Bool("verbose") }

type flagValue struct {
	ctx    *Context
	flag   *Flag
	set    bool
	values []string
}

func (v *flagValue) String() string {
	if len(v.values) > 0 {
		return v.values[len(v.values)-1]
	}
	return v.flag.Default
}

func (v *flagValue) Type() string {
	if v.flag.Kind == Bool {
		return "bool"
	}
	return "string"
}

func (v *flagValue) fail(e *usageError) error {
	if v.ctx.err == nil {
		v.ctx.err = e
	}
	return e
}

func (v *flagValue) Set(raw string) error {
	f := v.flag
	if v.set && f.Kind != Strings {
		return v.fail(&usageError{msg: fmt.Sprintf("the argument '%s' cannot be used multiple times", f.display()), usage: v.ctx.Cmd.usageLines()})
	}
	switch f.Kind {
	case Bool:
		if raw != "true" {
			return v.fail(&usageError{msg: fmt.Sprintf("unexpected value '%s' for '--%s' found; no more were expected", raw, f.Long), usage: v.ctx.smartUsage(f.Long, "")})
		}
	case Uint:
		bits := f.Bits
		if bits == 0 {
			bits = 64
		}
		if _, err := parseRustInt(raw, false, bits); err != nil {
			return v.fail(&usageError{msg: fmt.Sprintf("invalid value '%s' for '%s': %s", raw, f.display(), err)})
		}
	case Int:
		if _, err := parseRustInt(raw, true, 64); err != nil {
			return v.fail(&usageError{msg: fmt.Sprintf("invalid value '%s' for '%s': %s", raw, f.display(), err)})
		}
	case Range:
		// clap's ranged parser reads an i64, then checks the range.
		n, err := parseRustInt(raw, true, 64)
		if err != nil {
			return v.fail(&usageError{msg: fmt.Sprintf("invalid value '%s' for '%s': %s", raw, f.display(), err)})
		}
		if n < int64(f.Min) || n > int64(f.Max) {
			return v.fail(&usageError{msg: fmt.Sprintf("invalid value '%s' for '%s': %d is not in %d..=%d", raw, f.display(), n, f.Min, f.Max)})
		}
	case Enum:
		ok := false
		for _, p := range f.PossibleValue {
			ok = ok || p == raw
		}
		if !ok {
			msg := fmt.Sprintf("invalid value '%s' for '%s'\n  [possible values: %s]", raw, f.display(), strings.Join(f.PossibleValue, ", "))
			return v.fail(&usageError{msg: msg})
		}
	}
	v.set = true
	v.values = append(v.values, raw)
	v.ctx.used = append(v.ctx.used, f.Long)
	return nil
}

// parseRustInt mirrors Rust's integer FromStr error messages.
func parseRustInt(raw string, signed bool, bits int) (int64, error) {
	if raw == "" {
		return 0, errors.New("cannot parse integer from empty string")
	}
	s, negative := raw, false
	if s[0] == '+' || (signed && s[0] == '-') {
		negative = s[0] == '-'
		s = s[1:]
	}
	if s == "" {
		return 0, errors.New("invalid digit found in string")
	}
	for _, ch := range s {
		if ch < '0' || ch > '9' {
			return 0, errors.New("invalid digit found in string")
		}
	}
	if !signed {
		n, err := strconv.ParseUint(s, 10, bits)
		if err != nil {
			return 0, errors.New("number too large to fit in target type")
		}
		return int64(n), nil
	}
	n, err := strconv.ParseInt(raw, 10, bits)
	if err != nil {
		if negative {
			return 0, errors.New("number too small to fit in target type")
		}
		return 0, errors.New("number too large to fit in target type")
	}
	return n, nil
}

// smartUsage mirrors clap's error usage: required args, the args the user
// gave (minus a conflicting partner), then positionals.
func (c *Context) smartUsage(include, exclude string) []string {
	cmd := c.Cmd
	parts := []string{"uniclip" + strings.TrimPrefix(cmd.path(), "uniclip")}
	seen := map[string]bool{}
	add := func(long string) {
		if long == exclude || seen[long] {
			return
		}
		if g := cmd.groupOf(long); g != nil && g.Required {
			return // rendered as the group token below
		}
		if f := cmd.findFlag(long); f != nil {
			seen[long] = true
			parts = append(parts, f.display())
		}
	}
	for _, f := range cmd.sortedFlags() {
		if f.Required {
			add(f.Long)
		}
	}
	for _, long := range c.used {
		add(long)
	}
	if include != "" {
		add(include)
	}
	for i := range cmd.Groups {
		if cmd.Groups[i].Required {
			parts = append(parts, cmd.groupDisplay(&cmd.Groups[i]))
		}
	}
	for i, a := range cmd.Args {
		if i < len(c.Args) {
			parts = append(parts, "<"+a.Name+">")
		} else {
			parts = append(parts, a.display())
		}
	}
	return []string{strings.Join(parts, " ")}
}

func (c *Context) validate() error {
	cmd := c.Cmd
	var missing []string
	for _, f := range cmd.sortedFlags() {
		if f.Required && !c.values[f.Long].set {
			missing = append(missing, f.display())
		}
	}
	for i, a := range cmd.Args {
		if a.Required && i >= len(c.Args) {
			missing = append(missing, a.display())
		}
	}
	for i := range cmd.Groups {
		g := &cmd.Groups[i]
		given := false
		for _, m := range g.Members {
			given = given || c.values[m].set
		}
		if g.Required && !given {
			missing = append(missing, cmd.groupDisplay(g))
		}
	}
	if len(missing) > 0 {
		return &usageError{msg: "the following required arguments were not provided:\n  " + strings.Join(missing, "\n  "), usage: c.smartUsage("", "")}
	}
	// Conflicts are symmetric; report from the first given argument.
	given := append([]string{}, c.used...)
	for i := range cmd.Args {
		if i < len(c.Args) {
			given = append(given, "arg:"+cmd.Args[i].Name)
		}
	}
	for _, a := range given {
		for _, b := range given {
			if a != b && c.conflicts(a, b) {
				return &usageError{msg: fmt.Sprintf("the argument '%s' cannot be used with '%s'", c.displayOf(a), c.displayOf(b)), usage: c.smartUsage("", b)}
			}
		}
	}
	for _, long := range c.used {
		for _, req := range cmd.findFlag(long).Requires {
			if !c.values[req].set {
				usage := []string{"uniclip" + strings.TrimPrefix(cmd.path(), "uniclip") + " " + cmd.findFlag(req).display() + " " + cmd.findFlag(long).display()}
				return &usageError{msg: "the following required arguments were not provided:\n  " + cmd.findFlag(req).display(), usage: usage}
			}
		}
	}
	return nil
}

func (c *Context) conflictList(id string) []string {
	if name, ok := strings.CutPrefix(id, "arg:"); ok {
		for _, a := range c.Cmd.Args {
			if a.Name == name {
				return a.ConflictsWith
			}
		}
		return nil
	}
	return c.Cmd.findFlag(id).ConflictsWith
}

func (c *Context) conflicts(a, b string) bool {
	norm := func(id string) string { return strings.TrimPrefix(id, "arg:") }
	if ga := c.Cmd.groupOf(a); ga != nil && ga == c.Cmd.groupOf(b) {
		return true
	}
	for _, x := range c.conflictList(a) {
		if x == norm(b) {
			return true
		}
	}
	for _, x := range c.conflictList(b) {
		if x == norm(a) {
			return true
		}
	}
	return false
}

func (c *Context) displayOf(id string) string {
	if name, ok := strings.CutPrefix(id, "arg:"); ok {
		for _, a := range c.Cmd.Args {
			if a.Name == name {
				return a.display()
			}
		}
	}
	return c.Cmd.findFlag(id).display()
}

var (
	reNeedsArg  = regexp.MustCompile(`^flag needs an argument: (?:--(\S+)|'(.)' in -\S+)$`)
	reUnknown   = regexp.MustCompile(`^unknown flag: (--\S+)$`)
	reUnknownSh = regexp.MustCompile(`^unknown shorthand flag: '(.)' in -\S*$`)
	reBadSyntax = regexp.MustCompile(`^bad flag syntax: (\S+)$`)
)

// Execute parses os.Args against root and runs the selected command,
// returning the process exit code.
func Execute(root *Command) int {
	link(root)
	ctxs := map[*cobra.Command]*Context{}
	var exitCode int
	cobraRoot := build(root, ctxs, &exitCode)
	cobraRoot.SetArgs(os.Args[1:])
	cobraRoot.SetOut(os.Stdout)
	cobraRoot.SetErr(os.Stderr)
	executed, err := cobraRoot.ExecuteC()
	if err != nil {
		ctx := ctxs[executed]
		rootErr := ctxs[cobraRoot].err
		var ue *usageError
		switch {
		case ctx != nil && ctx.err != nil:
			ue = ctx.err
		case rootErr != nil:
			ue = rootErr
		case errors.As(err, &ue):
		default:
			ue = translate(err, ctx)
		}
		fmt.Fprint(os.Stderr, ue.render())
		return 2
	}
	return exitCode
}

func translate(err error, ctx *Context) *usageError {
	msg := err.Error()
	var usage []string
	var cmd *Command
	if ctx != nil {
		cmd = ctx.Cmd
		usage = cmd.usageLines()
	}
	if m := reNeedsArg.FindStringSubmatch(msg); m != nil && cmd != nil {
		for _, f := range cmd.allFlags() {
			if f.Long == m[1] || (m[2] != "" && string(f.Short) == m[2]) {
				return &usageError{msg: fmt.Sprintf("a value is required for '%s' but none was supplied", f.display())}
			}
		}
	}
	arg := ""
	if m := reUnknown.FindStringSubmatch(msg); m != nil {
		arg = m[1]
	} else if m := reUnknownSh.FindStringSubmatch(msg); m != nil {
		arg = "-" + m[1]
	} else if m := reBadSyntax.FindStringSubmatch(msg); m != nil {
		arg = m[1]
	}
	if arg != "" {
		return &usageError{msg: fmt.Sprintf("unexpected argument '%s' found", arg), tip: fmt.Sprintf("to pass '%s' as a value, use '-- %s'", arg, arg), usage: usage}
	}
	return &usageError{msg: msg, usage: usage}
}

func wantsLongHelp() bool {
	for _, a := range os.Args[1:] {
		if a == "--" {
			return false
		}
		if a == "--help" {
			return true
		}
	}
	return false
}

func build(spec *Command, ctxs map[*cobra.Command]*Context, exitCode *int) *cobra.Command {
	ctx := &Context{Cmd: spec, values: map[string]*flagValue{}}
	cmd := &cobra.Command{
		Use:                spec.Name,
		Aliases:            spec.Aliases,
		Hidden:             spec.Hidden,
		SilenceErrors:      true,
		SilenceUsage:       true,
		DisableSuggestions: true,
		CompletionOptions:  cobra.CompletionOptions{DisableDefaultCmd: true},
	}
	ctxs[cmd] = ctx
	flags := cmd.Flags()
	if spec.parent == nil {
		flags = cmd.PersistentFlags()
	}
	flags.SortFlags = false
	cmd.Flags().SortFlags = false
	register := func(set *pflag.FlagSet, f *Flag) {
		v := &flagValue{ctx: ctx, flag: f}
		ctx.values[f.Long] = v
		short := ""
		if f.Short != 0 {
			short = string(f.Short)
		}
		pf := set.VarPF(v, f.Long, short, f.Help)
		if f.Kind == Bool {
			pf.NoOptDefVal = "true"
		}
	}
	for _, f := range spec.Flags {
		if f.Global {
			register(cmd.PersistentFlags(), f)
		} else {
			register(cmd.Flags(), f)
		}
	}
	// Inherited globals are parsed by the root's persistent flag set; mirror
	// their parsed state into this context after parsing.
	cmd.Flags().BoolP("help", "h", false, "Print help")
	if spec.parent == nil {
		cmd.Version = buildinfo.PackageVersion
		cmd.SetVersionTemplate("uniclip {{.Version}}\n")
		cmd.Flags().BoolP("version", "V", false, "Print version")
		cmd.SetHelpCommand(helpCommand(spec))
	} else if len(spec.Subs) > 0 {
		cmd.AddCommand(helpCommand(spec))
	}
	cmd.SetHelpFunc(func(c *cobra.Command, _ []string) {
		target := ctxs[c]
		if target == nil {
			target = ctx
		}
		fmt.Fprint(os.Stdout, target.Cmd.Help(wantsLongHelp()))
	})
	cmd.SetFlagErrorFunc(func(c *cobra.Command, err error) error { return err })
	cmd.Args = func(_ *cobra.Command, args []string) error {
		if len(args) > len(spec.Args) {
			extra := args[len(spec.Args)]
			if len(spec.Subs) > 0 && len(spec.Args) == 0 {
				return &usageError{msg: fmt.Sprintf("unrecognized subcommand '%s'", extra), usage: spec.usageLines()}
			}
			return &usageError{msg: fmt.Sprintf("unexpected argument '%s' found", extra), usage: spec.usageLines()}
		}
		return nil
	}
	cmd.RunE = func(c *cobra.Command, args []string) error {
		root := spec.root()
		for _, g := range root.Flags {
			if g.Global {
				if v := rootCtx(ctxs, c).values[g.Long]; v != nil {
					ctx.values[g.Long] = v
				}
			}
		}
		ctx.Args = args
		if err := ctx.validate(); err != nil {
			return err
		}
		if before := spec.root().Before; before != nil {
			before(ctx)
		}
		if spec.Run == nil {
			if spec.parent == nil {
				// `uniclip` alone prints help plus a blank line, exit 0.
				fmt.Fprint(os.Stdout, spec.Help(false)+"\n")
				return nil
			}
			fmt.Fprint(os.Stderr, spec.Help(false))
			*exitCode = 2
			return nil
		}
		*exitCode = spec.Run(ctx)
		return nil
	}
	for _, s := range spec.Subs {
		cmd.AddCommand(build(s, ctxs, exitCode))
	}
	return cmd
}

func rootCtx(ctxs map[*cobra.Command]*Context, c *cobra.Command) *Context {
	return ctxs[c.Root()]
}

// helpCommand implements `help [COMMAND]...`, printing the long help of the
// addressed command like clap.
func helpCommand(spec *Command) *cobra.Command {
	return &cobra.Command{
		Use:    "help",
		Hidden: true,
		Args:   cobra.ArbitraryArgs,
		RunE: func(_ *cobra.Command, args []string) error {
			target := spec
			for _, name := range args {
				var next *Command
				for _, s := range target.Subs {
					if s.Name == name || contains(s.Aliases, name) {
						next = s
					}
				}
				if next == nil {
					return &usageError{msg: fmt.Sprintf("unrecognized subcommand '%s'", name), usage: spec.usageLines()}
				}
				target = next
			}
			fmt.Fprint(os.Stdout, target.Help(true))
			return nil
		},
	}
}

func contains(list []string, s string) bool {
	for _, x := range list {
		if x == s {
			return true
		}
	}
	return false
}
