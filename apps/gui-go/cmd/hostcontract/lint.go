package main

import (
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/hostapi"
)

// constTokens maps the constant names of hostapi (CodeNotFound, UnlockWrongPassphrase, ...) to their wire tokens,
// read from the source so the lint never keeps a second list.
func constTokens(root string) map[string]string {
	fset := token.NewFileSet()
	file, err := parser.ParseFile(fset, filepath.Join(root, "apps/gui-go/internal/hostapi/errors.go"), nil, 0)
	if err != nil {
		fatal(err)
	}
	out := map[string]string{}
	for _, decl := range file.Decls {
		gen, ok := decl.(*ast.GenDecl)
		if !ok || gen.Tok != token.CONST {
			continue
		}
		for _, spec := range gen.Specs {
			value := spec.(*ast.ValueSpec)
			for i, name := range value.Names {
				if i < len(value.Values) {
					if lit, ok := value.Values[i].(*ast.BasicLit); ok && lit.Kind == token.STRING {
						out[name.Name] = strings.Trim(lit.Value, `"`)
					}
				}
			}
		}
	}
	return out
}

func tokensOf(infos []hostapi.CodeInfo) map[string]bool {
	out := map[string]bool{}
	for _, info := range infos {
		out[info.Token] = true
	}
	return out
}

// lint checks the Go sources against the host contract rules and returns the violations.
func lint(root string) []string {
	commands, problems := scanCommands(root)
	consts := constTokens(root)
	family := map[string]map[string]bool{
		"command": tokensOf(hostapi.Catalog.Command),
		"unlock":  tokensOf(hostapi.Catalog.Unlock),
		"config":  tokensOf(hostapi.Catalog.Config),
	}
	wires := map[string]string{}
	for _, c := range commands {
		if other, dup := wires[c.Wire]; dup {
			problems = append(problems, fmt.Sprintf("%s and %s have the same command name %s", other, c.Method, c.Wire))
		}
		wires[c.Wire] = c.Method
		declared := map[string]bool{}
		for _, code := range c.Codes {
			declared[code] = true
			if tokens, ok := family[c.Errors]; ok && !tokens[code] {
				problems = append(problems, fmt.Sprintf("%s declares %s, which is not in the %s error catalog (apps/gui-go/internal/hostapi)", c.Method, code, c.Errors))
			}
		}
		if (c.Errors == "text" || c.Errors == "none") && len(c.Codes) > 0 {
			problems = append(problems, fmt.Sprintf("%s: //uc:errors %s takes no codes", c.Method, c.Errors))
		}
		for _, name := range c.BodyCode {
			token, ok := consts[name]
			if !ok {
				continue
			}
			if !declared[token] && !(c.Errors == "unlock") {
				problems = append(problems, fmt.Sprintf("%s returns %s (hostapi.%s) but //uc:errors does not declare it", c.Method, token, name))
			}
		}
		if c.Errors != "none" && !hasError(c) {
			problems = append(problems, fmt.Sprintf("%s declares errors but returns no error", c.Method))
		}
		if c.Errors == "none" && hasError(c) && len(c.Codes) == 0 {
			// A command that can fail must say how: a method returning error with //uc:errors none is a gap.
			problems = append(problems, fmt.Sprintf("%s returns an error but //uc:errors is none", c.Method))
		}
	}
	problems = append(problems, lintSources(root)...)
	sort.Strings(problems)
	return problems
}

func hasError(c command) bool {
	for _, r := range c.Results {
		if r == "error" {
			return true
		}
	}
	return false
}

// lintSources scans every Go file of the host for rule violations that need no type information:
//   - an error code written as a string literal instead of a hostapi constant;
//   - an event name written as a string literal in emit / Event.On / Event.Emit instead of a host_events.go constant.
func lintSources(root string) []string {
	dir := filepath.Join(root, "apps/gui-go")
	entries, err := os.ReadDir(dir)
	if err != nil {
		fatal(err)
	}
	fset := token.NewFileSet()
	var problems []string
	for _, entry := range entries {
		name := entry.Name()
		if entry.IsDir() || !strings.HasSuffix(name, ".go") || strings.HasSuffix(name, "_test.go") {
			continue
		}
		file, err := parser.ParseFile(fset, filepath.Join(dir, name), nil, 0)
		if err != nil {
			fatal(err)
		}
		ast.Inspect(file, func(n ast.Node) bool {
			switch node := n.(type) {
			case *ast.CallExpr:
				sel, ok := node.Fun.(*ast.SelectorExpr)
				if !ok || len(node.Args) == 0 {
					return true
				}
				if pkg, ok := sel.X.(*ast.Ident); ok && pkg.Name == "hostapi" && sel.Sel.Name == "New" {
					if lit, ok := node.Args[0].(*ast.BasicLit); ok {
						problems = append(problems, fmt.Sprintf("%s: hostapi.New with the literal code %s: use a hostapi.Code constant", fset.Position(lit.Pos()), lit.Value))
					}
				}
				switch sel.Sel.Name {
				case "emit", "Emit", "On", "Once":
					if lit, ok := node.Args[0].(*ast.BasicLit); ok && lit.Kind == token.STRING && isEventReceiver(sel) {
						problems = append(problems, fmt.Sprintf("%s: event name %s as a literal: declare it in host_events.go", fset.Position(lit.Pos()), lit.Value))
					}
				}
			case *ast.CompositeLit:
				typ, ok := node.Type.(*ast.SelectorExpr)
				if !ok {
					return true
				}
				if pkg, ok := typ.X.(*ast.Ident); !ok || pkg.Name != "hostapi" {
					return true
				}
				switch typ.Sel.Name {
				case "CommandError", "UnlockError", "ConfigError":
					for _, elt := range node.Elts {
						kv, ok := elt.(*ast.KeyValueExpr)
						if !ok {
							continue
						}
						key, _ := kv.Key.(*ast.Ident)
						if key == nil || (key.Name != "Code" && key.Name != "Kind") {
							continue
						}
						if lit, ok := kv.Value.(*ast.BasicLit); ok {
							problems = append(problems, fmt.Sprintf("%s: hostapi.%s with the literal %s %s: use a hostapi constant", fset.Position(lit.Pos()), typ.Sel.Name, key.Name, lit.Value))
						}
					}
				}
			}
			return true
		})
	}
	return problems
}

// isEventReceiver limits the event-name rule to the host's emit helper and the Wails event manager.
func isEventReceiver(sel *ast.SelectorExpr) bool {
	switch x := sel.X.(type) {
	case *ast.Ident:
		return sel.Sel.Name == "emit"
	case *ast.SelectorExpr:
		return x.Sel.Name == "Event"
	}
	return false
}
