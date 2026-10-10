package main

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"
)

const (
	docsPath   = "docs/architecture/gui-go-host-commands.md"
	docsBegin  = "<!-- BEGIN GENERATED: host-contract -->"
	docsEnd    = "<!-- END GENERATED: host-contract -->"
	eventsPath = "apps/gui-go/host_events.go"
)

var osLabel = map[string]string{"real": "真实", "noop": "有意空操作", "unsupported": "不支持"}

type event struct {
	Const     string
	Name      string
	Direction string
	Payload   string
	Note      string
}

var (
	eventConstRe    = regexp.MustCompile(`(?m)^\s*(\w+)\s*=\s*"([^"]+)"\s*//\s*(.*)$`)
	eventRegisterRe = regexp.MustCompile(`RegisterEvent\[([^\]]*)\]\((\w+)\)`)
)

// scanEvents reads host_events.go: the constants (name and the direction/payload comment) and the typed registrations.
func scanEvents(root string) []event {
	raw, err := os.ReadFile(filepath.Join(root, eventsPath))
	if err != nil {
		fatal(err)
	}
	source := string(raw)
	payloads := map[string]string{}
	for _, m := range eventRegisterRe.FindAllStringSubmatch(source, -1) {
		payloads[m[2]] = strings.Replace(m[1], "application.Void", "无载荷", 1)
	}
	var events []event
	for _, m := range eventConstRe.FindAllStringSubmatch(source, -1) {
		direction := "宿主 → 页面"
		switch {
		case strings.Contains(m[3], "page -> host"):
			direction = "页面 → 宿主"
		case strings.Contains(m[3], "both ways"):
			direction = "双向"
		}
		events = append(events, event{Const: m[1], Name: m[2], Direction: direction, Payload: payloads[m[1]], Note: m[3]})
	}
	return events
}

func cell(text string) string {
	return strings.ReplaceAll(strings.ReplaceAll(text, "|", `\|`), "\n", " ")
}

func renderDocsBlock(commands []command, events []event) string {
	var b strings.Builder
	b.WriteString(docsBegin + "\n\n")
	b.WriteString("此区块由 `go run ./cmd/hostcontract docs` 从 Go 源码生成（命令来自 `*HostService` 的导出方法及其 `//uc:` 指令，事件来自 `apps/gui-go/host_events.go`），请勿手改。\n\n")
	fmt.Fprintf(&b, "### 命令（%d 个）\n\n", len(commands))
	b.WriteString("| 命令 | Go 方法 | 参数 | 结果 | 错误族 / 错误码 | macOS | Windows | Linux | 来源 / 说明 |\n")
	b.WriteString("| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n")
	for _, c := range commands {
		params := make([]string, len(c.Params))
		for i, p := range c.Params {
			params[i] = p.Name + " " + p.Type
		}
		paramText := "-"
		if len(params) > 0 {
			paramText = "`" + strings.Join(params, ", ") + "`"
		}
		results := "-"
		var typed []string
		for _, r := range c.Results {
			if r != "error" {
				typed = append(typed, r)
			}
		}
		if len(typed) > 0 {
			results = strings.Join(typed, ", ")
		}
		errs := c.Errors
		if len(c.Codes) > 0 {
			errs += ": " + strings.Join(c.Codes, ", ")
		}
		source := "契约命令"
		if c.Adapter != "" {
			source = "适配器：`" + c.Adapter + "`"
		}
		doc := c.Doc
		fmt.Fprintf(&b, "| `%s` | `%s` | %s | %s | %s | %s | %s | %s | %s；%s |\n",
			c.Wire, c.Method, cell(paramText), cell("`"+results+"`"), cell(errs),
			osLabel[c.OS["darwin"]], osLabel[c.OS["windows"]], osLabel[c.OS["linux"]], source, cell(firstSentence(doc)))
	}
	b.WriteString("\n### 事件\n\n")
	b.WriteString("| 事件 | Go 常量 | 方向 | 载荷类型 |\n| --- | --- | --- | --- |\n")
	for _, e := range events {
		fmt.Fprintf(&b, "| `%s` | `%s` | %s | `%s` |\n", e.Name, e.Const, e.Direction, e.Payload)
	}
	b.WriteString("\n" + docsEnd)
	return b.String()
}

// firstSentence keeps the first sentence of the method's doc comment (English, as in the source).
func firstSentence(doc string) string {
	doc = strings.Join(strings.Fields(doc), " ")
	if i := strings.Index(doc, ". "); i > 0 {
		return doc[:i+1]
	}
	return doc
}

// renderDocs replaces the generated block of the committed document, keeping the hand-written prose around it.
func renderDocs(root string, commands []command, events []event) string {
	raw, err := os.ReadFile(filepath.Join(root, docsPath))
	if err != nil {
		fatal(err)
	}
	text := string(raw)
	begin, end := strings.Index(text, docsBegin), strings.Index(text, docsEnd)
	if begin < 0 || end < begin {
		fatal(fmt.Errorf("%s lacks the generated block markers", docsPath))
	}
	return text[:begin] + renderDocsBlock(commands, events) + text[end+len(docsEnd):]
}
