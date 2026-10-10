package main

import (
	"fmt"
	"go/ast"
	"go/format"
	"go/parser"
	"go/token"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"unicode"
)

// A command is one exported method of *HostService: the Go source is the contract, so everything here is read from
// the AST of apps/gui-go/*.go (all build tags: the exported method set must not depend on the platform).

type param struct {
	Name string
	Type string
}

type command struct {
	Method   string // Go method name, e.g. GetContentUnlocked
	Wire     string // snake_case command name, e.g. get_content_unlocked
	File     string // repo-relative source file
	Line     int
	Doc      string  // doc comment without directives
	Params   []param // without the context.Context
	Results  []string
	HasCtx   bool
	Returns  bool     // returns something besides error
	Errors   string   // //uc:errors family
	Codes    []string // //uc:errors codes
	OS       map[string]string
	Adapter  string // //uc:adapter frontend host module (e.g. @/host/opener), empty for a command the pages call directly
	BodyCode []string
}

var osNames = []string{"darwin", "windows", "linux"}
var osStatuses = map[string]bool{"real": true, "noop": true, "unsupported": true}
var errorFamilies = map[string]bool{"command": true, "unlock": true, "config": true, "text": true, "none": true}

// wireName converts a Go method name to the command's snake_case name. Initialisms (ID, URL) stay one word.
func wireName(method string) string {
	runes := []rune(method)
	var words []string
	start := 0
	for i := 1; i < len(runes); i++ {
		prev, cur := runes[i-1], runes[i]
		nextLower := i+1 < len(runes) && unicode.IsLower(runes[i+1])
		if (unicode.IsLower(prev) && unicode.IsUpper(cur)) || (unicode.IsUpper(prev) && unicode.IsUpper(cur) && nextLower) {
			words = append(words, strings.ToLower(string(runes[start:i])))
			start = i
		}
	}
	words = append(words, strings.ToLower(string(runes[start:])))
	return strings.Join(words, "_")
}

func exprString(fset *token.FileSet, expr ast.Expr) string {
	var b strings.Builder
	if err := format.Node(&b, fset, expr); err != nil {
		return "?"
	}
	return b.String()
}

// scanCommands reads every exported method of *HostService in dir.
func scanCommands(root string) ([]command, []string) {
	dir := filepath.Join(root, "apps/gui-go")
	entries, err := os.ReadDir(dir)
	if err != nil {
		fatal(err)
	}
	fset := token.NewFileSet()
	var commands []command
	var problems []string
	for _, entry := range entries {
		name := entry.Name()
		if entry.IsDir() || !strings.HasSuffix(name, ".go") || strings.HasSuffix(name, "_test.go") {
			continue
		}
		file, err := parser.ParseFile(fset, filepath.Join(dir, name), nil, parser.ParseComments)
		if err != nil {
			fatal(err)
		}
		for _, decl := range file.Decls {
			fn, ok := decl.(*ast.FuncDecl)
			if !ok || fn.Recv == nil || !fn.Name.IsExported() || !receiverIs(fn, "HostService") {
				continue
			}
			c := command{Method: fn.Name.Name, Wire: wireName(fn.Name.Name), File: "apps/gui-go/" + name, Line: fset.Position(fn.Pos()).Line, OS: map[string]string{}}
			c.parseDirectives(fn, &problems)
			for _, field := range fn.Type.Params.List {
				typ := exprString(fset, field.Type)
				if typ == "context.Context" {
					c.HasCtx = true
					continue
				}
				for _, ident := range field.Names {
					c.Params = append(c.Params, param{Name: ident.Name, Type: typ})
				}
			}
			if fn.Type.Results != nil {
				for _, field := range fn.Type.Results.List {
					typ := exprString(fset, field.Type)
					n := len(field.Names)
					if n == 0 {
						n = 1
					}
					for i := 0; i < n; i++ {
						c.Results = append(c.Results, typ)
					}
				}
			}
			for _, r := range c.Results {
				if r != "error" {
					c.Returns = true
				}
			}
			c.BodyCode = hostapiCodes(fn)
			commands = append(commands, c)
		}
	}
	sort.Slice(commands, func(i, j int) bool { return commands[i].Wire < commands[j].Wire })
	return commands, problems
}

func receiverIs(fn *ast.FuncDecl, typeName string) bool {
	if len(fn.Recv.List) != 1 {
		return false
	}
	star, ok := fn.Recv.List[0].Type.(*ast.StarExpr)
	if !ok {
		return false
	}
	ident, ok := star.X.(*ast.Ident)
	return ok && ident.Name == typeName
}

// parseDirectives reads the //uc: lines of the doc comment and keeps the prose.
func (c *command) parseDirectives(fn *ast.FuncDecl, problems *[]string) {
	if fn.Doc == nil {
		*problems = append(*problems, fmt.Sprintf("%s (%s:%d): no doc comment", c.Method, c.File, c.Line))
		return
	}
	var prose []string
	for _, comment := range fn.Doc.List {
		text := strings.TrimPrefix(comment.Text, "//")
		if !strings.HasPrefix(text, "uc:") {
			prose = append(prose, strings.TrimPrefix(text, " "))
			continue
		}
		fields := strings.Fields(strings.TrimPrefix(text, "uc:"))
		if len(fields) == 0 {
			continue
		}
		switch fields[0] {
		case "errors":
			if len(fields) < 2 || !errorFamilies[fields[1]] {
				*problems = append(*problems, fmt.Sprintf("%s: //uc:errors needs a family (command|unlock|config|text|none)", c.Method))
				continue
			}
			c.Errors, c.Codes = fields[1], fields[2:]
		case "os":
			for _, pair := range fields[1:] {
				key, status, ok := strings.Cut(pair, "=")
				if !ok || !osStatuses[status] {
					*problems = append(*problems, fmt.Sprintf("%s: bad //uc:os entry %q (want darwin|windows|linux|all=real|noop|unsupported)", c.Method, pair))
					continue
				}
				if key == "all" {
					for _, name := range osNames {
						c.OS[name] = status
					}
				} else {
					c.OS[key] = status
				}
			}
		case "adapter":
			if len(fields) != 2 {
				*problems = append(*problems, fmt.Sprintf("%s: //uc:adapter needs the frontend host module", c.Method))
				continue
			}
			c.Adapter = fields[1]
		default:
			*problems = append(*problems, fmt.Sprintf("%s: unknown directive //uc:%s", c.Method, fields[0]))
		}
	}
	c.Doc = strings.TrimSpace(strings.Join(prose, "\n"))
	if c.Errors == "" {
		*problems = append(*problems, fmt.Sprintf("%s (%s:%d): missing //uc:errors", c.Method, c.File, c.Line))
	}
	for _, name := range osNames {
		if c.OS[name] == "" {
			*problems = append(*problems, fmt.Sprintf("%s (%s:%d): missing //uc:os for %s", c.Method, c.File, c.Line, name))
		}
	}
}

// hostapiCodes lists the hostapi.Code* / hostapi.Unlock* constants the method body names.
func hostapiCodes(fn *ast.FuncDecl) []string {
	var codes []string
	if fn.Body == nil {
		return nil
	}
	ast.Inspect(fn.Body, func(n ast.Node) bool {
		sel, ok := n.(*ast.SelectorExpr)
		if !ok {
			return true
		}
		if pkg, ok := sel.X.(*ast.Ident); ok && pkg.Name == "hostapi" && (strings.HasPrefix(sel.Sel.Name, "Code") || strings.HasPrefix(sel.Sel.Name, "Unlock")) {
			codes = append(codes, sel.Sel.Name)
		}
		return true
	})
	return codes
}
