// Command docscheck enforces the documentation lifecycle (see docs/decisions/README.md).
//
// Errors (exit 1):
//   - live docs, code or tooling link to a file under docs/decisions/archive/;
//   - relative markdown links that do not resolve (archive files are exempt);
//   - architecture pages without valid front matter (paths exist, verified commit exists);
//   - archived decisions without a "落地" line linking existing live docs;
//   - decision files missing from the ledger in docs/decisions/README.md.
//
// Warnings (errors with -strict):
//   - an architecture page whose code paths changed since its `verified` commit.
//
// -bump <page> sets `verified` of an architecture page to HEAD after it has been re-checked against the code.
package main

import (
	"bufio"
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
)

var (
	errs  []string
	warns []string
)

func errorf(format string, a ...any) { errs = append(errs, fmt.Sprintf(format, a...)) }
func warnf(format string, a ...any)  { warns = append(warns, fmt.Sprintf(format, a...)) }

func main() {
	strict := flag.Bool("strict", false, "treat staleness warnings as errors")
	bump := flag.String("bump", "", "architecture page whose `verified` should be set to HEAD")
	flag.Parse()

	root, err := gitOut("rev-parse", "--show-toplevel")
	if err != nil {
		fmt.Fprintln(os.Stderr, "docscheck: not in a git repository:", err)
		os.Exit(2)
	}
	if err := os.Chdir(root); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(2)
	}

	if *bump != "" {
		if err := bumpPage(*bump); err != nil {
			fmt.Fprintln(os.Stderr, "docscheck:", err)
			os.Exit(1)
		}
		return
	}

	checkArchiveRefs()
	checkLinks()
	checkArchitecture()
	checkDecisions()

	sort.Strings(errs)
	sort.Strings(warns)
	for _, w := range warns {
		fmt.Println("warn: ", w)
	}
	for _, e := range errs {
		fmt.Println("error:", e)
	}
	if len(errs) > 0 || (*strict && len(warns) > 0) {
		os.Exit(1)
	}
	fmt.Printf("docscheck: ok (%d warnings)\n", len(warns))
}

// ---------- helpers ----------

func gitOut(args ...string) (string, error) {
	out, err := exec.Command("git", args...).Output()
	return strings.TrimSpace(string(out)), err
}

func walk(fn func(path string)) {
	skipDirs := map[string]bool{".git": true, "dist": true, "bin": true, "node_modules": true, ".build": true, ".cache": true}
	filepath.WalkDir(".", func(p string, d os.DirEntry, err error) error {
		if err != nil {
			return nil
		}
		if d.IsDir() {
			if skipDirs[d.Name()] {
				return filepath.SkipDir
			}
			return nil
		}
		fn(filepath.ToSlash(p))
		return nil
	})
}

func inArchive(p string) bool { return strings.HasPrefix(p, "docs/decisions/archive/") }

func readLines(p string) []string {
	f, err := os.Open(p)
	if err != nil {
		return nil
	}
	defer f.Close()
	var lines []string
	sc := bufio.NewScanner(f)
	sc.Buffer(make([]byte, 1<<20), 1<<20)
	for sc.Scan() {
		lines = append(lines, sc.Text())
	}
	return lines
}

// ---------- rule: nothing live may reference archived decisions ----------

var archiveRef = regexp.MustCompile(`decisions/archive/[A-Za-z0-9._-]+\.md`)

func checkArchiveRefs() {
	allowed := func(p string) bool {
		return inArchive(p) || strings.HasPrefix(p, "docs/decisions/") ||
			strings.HasPrefix(p, ".claude/skills/") || strings.HasPrefix(p, "tools/docscheck/")
	}
	walk(func(p string) {
		if allowed(p) || !textual(p) {
			return
		}
		for i, l := range readLines(p) {
			if m := archiveRef.FindString(l); m != "" {
				errorf("%s:%d references archived decision %s; link the living doc (docs/architecture or docs/contracts) instead", p, i+1, m)
			}
		}
	})
}

func textual(p string) bool {
	switch strings.ToLower(filepath.Ext(p)) {
	case ".md", ".go", ".swift", ".toml", ".yml", ".yaml", ".json", ".txt", ".lua", "":
		return true
	}
	return false
}

// ---------- rule: relative markdown links resolve ----------

var mdLink = regexp.MustCompile(`\]\(([^)\s]+)\)`)

func checkLinks() {
	walk(func(p string) {
		if !strings.HasSuffix(p, ".md") || inArchive(p) || strings.HasPrefix(p, ".claude/") || strings.HasPrefix(p, "app/") && strings.Contains(p, "/.build/") {
			return
		}
		inFence := false
		for i, l := range readLines(p) {
			if strings.HasPrefix(strings.TrimSpace(l), "```") {
				inFence = !inFence
			}
			if inFence {
				continue
			}
			for _, m := range mdLink.FindAllStringSubmatch(l, -1) {
				t := m[1]
				if strings.Contains(t, "://") || strings.HasPrefix(t, "#") || strings.HasPrefix(t, "mailto:") {
					continue
				}
				if j := strings.IndexAny(t, "#?"); j >= 0 {
					t = t[:j]
				}
				if t == "" {
					continue
				}
				var target string
				if strings.HasPrefix(t, "/") {
					target = strings.TrimPrefix(t, "/")
				} else {
					target = filepath.Join(filepath.Dir(p), t)
				}
				if _, err := os.Stat(target); err != nil {
					errorf("%s:%d broken link %s", p, i+1, m[1])
				}
			}
		}
	})
}

// ---------- rule: architecture pages ----------

type frontMatter struct {
	paths    []string
	verified string
	end      int // index of the closing --- line
}

func parseFrontMatter(lines []string) (*frontMatter, bool) {
	if len(lines) == 0 || strings.TrimSpace(lines[0]) != "---" {
		return nil, false
	}
	fm := &frontMatter{}
	section := ""
	for i := 1; i < len(lines); i++ {
		l := strings.TrimRight(lines[i], " \t")
		if l == "---" {
			fm.end = i
			return fm, true
		}
		switch {
		case strings.HasPrefix(l, "paths:"):
			section = "paths"
		case strings.HasPrefix(l, "verified:"):
			fm.verified = strings.TrimSpace(strings.TrimPrefix(l, "verified:"))
			section = ""
		case strings.HasPrefix(strings.TrimSpace(l), "- ") && section == "paths":
			fm.paths = append(fm.paths, strings.Trim(strings.TrimSpace(strings.TrimPrefix(strings.TrimSpace(l), "- ")), "`\"'"))
		case strings.TrimSpace(l) == "":
		default:
			section = ""
		}
	}
	return nil, false
}

func architecturePages() []string {
	var pages []string
	walk(func(p string) {
		if strings.HasPrefix(p, "docs/architecture/") && strings.HasSuffix(p, ".md") && filepath.Base(p) != "README.md" {
			pages = append(pages, p)
		}
	})
	return pages
}

func checkArchitecture() {
	for _, p := range architecturePages() {
		fm, ok := parseFrontMatter(readLines(p))
		if !ok {
			errorf("%s: missing front matter (--- paths: [...] verified: <commit> ---)", p)
			continue
		}
		if len(fm.paths) == 0 {
			errorf("%s: front matter has no `paths`", p)
		}
		for _, cp := range fm.paths {
			if _, err := os.Stat(cp); err != nil {
				errorf("%s: path %q in front matter does not exist", p, cp)
			}
		}
		if fm.verified == "" {
			errorf("%s: front matter has no `verified` commit", p)
			continue
		}
		if _, err := gitOut("cat-file", "-e", fm.verified+"^{commit}"); err != nil {
			errorf("%s: verified commit %q does not exist", p, fm.verified)
			continue
		}
		args := append([]string{"diff", "--name-only", fm.verified, "--"}, fm.paths...)
		out, err := gitOut(args...)
		if err == nil && out != "" {
			files := strings.Split(out, "\n")
			more := ""
			if len(files) > 3 {
				more = fmt.Sprintf(" (+%d more)", len(files)-3)
				files = files[:3]
			}
			warnf("%s: stale — code changed since %s: %s%s", p, short(fm.verified), strings.Join(files, ", "), more)
		}
	}
}

func short(s string) string {
	if len(s) > 9 {
		return s[:9]
	}
	return s
}

func bumpPage(page string) error {
	lines := readLines(page)
	fm, ok := parseFrontMatter(lines)
	if !ok {
		return fmt.Errorf("%s: no front matter", page)
	}
	if st, _ := gitOut(append([]string{"status", "--porcelain", "--"}, fm.paths...)...); st != "" {
		return fmt.Errorf("uncommitted changes under the page's paths; commit them first so `verified` names the code you checked:\n%s", st)
	}
	head, err := gitOut("rev-parse", "HEAD")
	if err != nil {
		return err
	}
	for i := 1; i < fm.end; i++ {
		if strings.HasPrefix(lines[i], "verified:") {
			lines[i] = "verified: " + head
		}
	}
	if err := os.WriteFile(page, []byte(strings.Join(lines, "\n")+"\n"), 0o644); err != nil {
		return err
	}
	fmt.Printf("%s: verified -> %s\n", page, short(head))
	return nil
}

// ---------- rule: decisions ----------

var decisionNum = regexp.MustCompile(`^(\d{4})-`)

func checkDecisions() {
	ledger := strings.Join(readLines("docs/decisions/README.md"), "\n")
	if ledger == "" {
		errorf("docs/decisions/README.md (the ledger) is missing")
	}
	for _, dir := range []string{"proposed", "archive"} {
		entries, _ := os.ReadDir(filepath.Join("docs/decisions", dir))
		for _, e := range entries {
			name := e.Name()
			if e.IsDir() || !strings.HasSuffix(name, ".md") || !decisionNum.MatchString(name) {
				continue
			}
			p := "docs/decisions/" + dir + "/" + name
			if !strings.Contains(ledger, dir+"/"+name) {
				errorf("%s is not listed in the ledger docs/decisions/README.md", p)
			}
			lines := readLines(p)
			status, landed := "", ""
			for _, l := range lines {
				if strings.HasPrefix(l, "- 状态") && status == "" {
					status = l
				}
				if strings.HasPrefix(l, "- 落地") && landed == "" {
					landed = l
				}
			}
			if dir == "archive" {
				if !strings.Contains(status, "已归档") {
					errorf("%s: status line must say 已归档", p)
				}
				ok := false
				for _, m := range mdLink.FindAllStringSubmatch(landed, -1) {
					t := m[1]
					if j := strings.Index(t, "#"); j >= 0 {
						t = t[:j]
					}
					full := filepath.ToSlash(filepath.Join(filepath.Dir(p), t))
					if _, err := os.Stat(full); err == nil &&
						(strings.HasPrefix(full, "docs/architecture/") || strings.HasPrefix(full, "docs/contracts/")) {
						ok = true
					}
				}
				if !ok {
					errorf("%s: archived decision needs a `- 落地：` line linking existing docs/architecture or docs/contracts pages", p)
				}
			} else if strings.Contains(status, "已归档") {
				errorf("%s: status says 已归档 but file is still in proposed/", p)
			}
		}
	}
}
