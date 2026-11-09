package e2e

import (
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"

	"github.com/rogpeppe/go-internal/txtar"
)

var update = flag.Bool("update", false, "rewrite CASES.md from the headers in testdata/")

// A case documents itself in the comment block at the top of its script:
//
//	# 用例: 一句话标题
//	# 目的: 为什么要有这个用例
//	# 流程:
//	#   1. ...
//	# 期望:
//	#   - ...
//	# 不应:
//	#   - ...
//
// CASES.md is generated from those blocks, so the catalog cannot drift from
// the scripts and nobody needs to read Go to know what a case checks.
var sections = []string{"用例", "目的", "流程", "期望", "不应"}

type caseDoc struct {
	file   string
	fields map[string][]string // section -> lines
	script string              // the commands after the header
}

func parseCase(path string) (caseDoc, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return caseDoc{}, err
	}
	a := txtar.Parse(data)
	c := caseDoc{file: filepath.Base(path), fields: map[string][]string{}}
	cur := ""
	var body []string
	inHeader := true
	for _, line := range strings.Split(string(a.Comment), "\n") {
		if inHeader && strings.HasPrefix(line, "#") {
			text := strings.TrimSpace(strings.TrimPrefix(line, "#"))
			matched := false
			for _, s := range sections {
				if rest, ok := strings.CutPrefix(text, s+":"); ok {
					cur, matched = s, true
					if rest = strings.TrimSpace(rest); rest != "" {
						c.fields[s] = append(c.fields[s], rest)
					}
				}
			}
			if !matched && cur != "" && text != "" {
				c.fields[cur] = append(c.fields[cur], text)
			}
			continue
		}
		if inHeader && strings.TrimSpace(line) == "" && len(c.fields) == 0 {
			continue
		}
		inHeader = false
		body = append(body, line)
	}
	c.script = strings.Trim(strings.Join(body, "\n"), "\n")
	for _, s := range sections {
		if len(c.fields[s]) == 0 {
			return c, fmt.Errorf("%s: header lacks %q (a case must say what it does, expects and must not see)", c.file, s+":")
		}
	}
	return c, nil
}

func render(cases []caseDoc) string {
	var b strings.Builder
	b.WriteString("# CLI 端到端用例\n\n")
	b.WriteString("> 本文件由 `go test ./e2e/cli -run TestCatalog -update` 根据 `testdata/*.txtar` 顶部的说明生成，不要手改。\n")
	b.WriteString("> 怎么运行、怎么新增用例见 [README.md](README.md)。\n\n")
	b.WriteString("| 用例 | 标题 |\n|---|---|\n")
	for _, c := range cases {
		name := strings.TrimSuffix(c.file, ".txtar")
		fmt.Fprintf(&b, "| [%s](#%s) | %s |\n", name, name, strings.Join(c.fields["用例"], " "))
	}
	b.WriteString("\n")
	for _, c := range cases {
		name := strings.TrimSuffix(c.file, ".txtar")
		fmt.Fprintf(&b, "## %s\n\n**%s**\n\n", name, strings.Join(c.fields["用例"], " "))
		fmt.Fprintf(&b, "运行：`UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/%s' -v`\n\n", name)
		for _, s := range sections[1:] {
			fmt.Fprintf(&b, "**%s**\n\n", s)
			for _, l := range c.fields[s] {
				if strings.HasPrefix(l, "- ") || (len(l) > 2 && l[0] >= '0' && l[0] <= '9') {
					b.WriteString(l + "\n")
				} else {
					b.WriteString(l + "\n\n")
				}
			}
			b.WriteString("\n")
		}
		fmt.Fprintf(&b, "<details><summary>脚本 testdata/%s</summary>\n\n```\n%s\n```\n\n</details>\n\n", c.file, c.script)
	}
	return b.String()
}

func TestCatalog(t *testing.T) {
	files, err := filepath.Glob("testdata/*.txtar")
	if err != nil || len(files) == 0 {
		t.Fatalf("no cases found: %v", err)
	}
	sort.Strings(files)
	var cases []caseDoc
	for _, f := range files {
		c, err := parseCase(f)
		if err != nil {
			t.Error(err)
			continue
		}
		cases = append(cases, c)
	}
	if t.Failed() {
		return
	}
	got := render(cases)
	if *update {
		if err := os.WriteFile("CASES.md", []byte(got), 0o644); err != nil {
			t.Fatal(err)
		}
		return
	}
	want, _ := os.ReadFile("CASES.md")
	if string(want) != got {
		t.Fatal("CASES.md is stale; run: go test ./e2e/cli -run TestCatalog -update")
	}
}
