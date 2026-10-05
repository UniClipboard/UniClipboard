package cli

import (
	"sort"
	"strings"
)

// jaro is strsim's Jaro similarity over Unicode scalar values, which clap
// uses for "did you mean" suggestions.
func jaro(a, b string) float64 {
	ar, br := []rune(a), []rune(b)
	if len(ar) == 0 && len(br) == 0 {
		return 1
	}
	if len(ar) == 0 || len(br) == 0 {
		return 0
	}
	searchRange := max(len(ar), len(br))/2 - 1
	if searchRange < 0 {
		searchRange = 0
	}
	aFlags, bFlags := make([]bool, len(ar)), make([]bool, len(br))
	matches := 0
	for i, ac := range ar {
		minBound := 0
		if i > searchRange {
			minBound = i - searchRange
		}
		maxBound := min(len(br), i+searchRange+1)
		for j := minBound; j < maxBound; j++ {
			if ac == br[j] && !bFlags[j] {
				aFlags[i], bFlags[j] = true, true
				matches++
				break
			}
		}
	}
	if matches == 0 {
		return 0
	}
	transpositions, k := 0, 0
	for i, ac := range ar {
		if !aFlags[i] {
			continue
		}
		for !bFlags[k] {
			k++
		}
		if ac != br[k] {
			transpositions++
		}
		k++
	}
	transpositions /= 2
	m := float64(matches)
	return (m/float64(len(ar)) + m/float64(len(br)) + float64(matches-transpositions)/m) / 3
}

// didYouMean returns candidates with similarity above 0.7 in ascending
// confidence order, like clap's `did_you_mean`.
func didYouMean(value string, candidates []string) []string {
	type scored struct {
		score float64
		name  string
	}
	var found []scored
	for _, c := range candidates {
		if s := jaro(value, c); s > 0.7 {
			found = append(found, scored{s, c})
		}
	}
	sort.SliceStable(found, func(i, j int) bool { return found[i].score < found[j].score })
	out := make([]string, len(found))
	for i, f := range found {
		out[i] = f.name
	}
	return out
}

// subcommandNames lists names and all aliases, plus `help`, as clap does.
func (c *Command) subcommandNames() []string {
	var names []string
	for _, s := range c.Subs {
		names = append(append(names, s.Name), s.Aliases...)
	}
	return append(names, "help")
}

func subcommandTip(candidates []string) string {
	quoted := make([]string, len(candidates))
	for i, c := range candidates {
		quoted[i] = "'" + c + "'"
	}
	if len(quoted) == 1 {
		return "a similar subcommand exists: " + quoted[0]
	}
	return "some similar subcommands exist: " + strings.Join(quoted, ", ")
}

// longNames lists the long flags known to the command (keymap), including
// help and, at the root, version.
func (c *Command) longNames() []string {
	var longs []string
	for _, f := range c.allFlags() {
		longs = append(longs, f.Long)
	}
	longs = append(longs, "help")
	if c.parent == nil {
		longs = append(longs, "version")
	}
	return longs
}
