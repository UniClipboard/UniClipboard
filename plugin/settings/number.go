package settings

import (
	"math"
	"strconv"
	"strings"
)

// Every shell uses binary64 numbers. Reject JSON spellings that would change
// their decimal value when converted to/from binary64, instead of rounding
// plugin configuration silently (especially int64-sized IDs and fractions).
func portableNumber(text string) bool {
	n, err := strconv.ParseFloat(text, 64)
	return err == nil && !math.IsInf(n, 0) && !math.IsNaN(n) &&
		decimalKey(text) == decimalKey(strconv.FormatFloat(n, 'g', -1, 64))
}

func decimalKey(text string) string {
	sign := ""
	if strings.HasPrefix(text, "-") {
		sign, text = "-", text[1:]
	}
	exponent := 0
	if pos := strings.IndexAny(text, "eE"); pos >= 0 {
		var err error
		exponent, err = strconv.Atoi(text[pos+1:])
		if err != nil || exponent < -10000 || exponent > 10000 {
			return "invalid"
		}
		text = text[:pos]
	}
	if pos := strings.IndexByte(text, '.'); pos >= 0 {
		exponent -= len(text) - pos - 1
		text = text[:pos] + text[pos+1:]
	}
	for _, c := range text {
		if c < '0' || c > '9' {
			return "invalid"
		}
	}
	text = strings.TrimLeft(text, "0")
	if text == "" {
		return "0"
	}
	trimmed := strings.TrimRight(text, "0")
	exponent += len(text) - len(trimmed)
	return sign + trimmed + "e" + strconv.Itoa(exponent)
}
