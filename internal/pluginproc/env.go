package pluginproc

import (
	"strings"
)

// buildEnv constructs the minimal environment for a plugin subprocess.
// It forwards only OS runtime, temporary-directory, TLS certificate, and
// outbound HTTP proxy variables from parent, applies caller-supplied extra
// entries, and enforces the protocol variables. Unrelated host variables and
// credentials are never inherited.
func buildEnv(goos string, parent, extra []string, dir, fallbackTemp string) []string {
	windows := goos == "windows"
	norm := func(k string) string {
		if windows {
			return strings.ToUpper(k)
		}
		return k
	}

	var out []string
	index := map[string]int{}
	set := func(k, v string) {
		nk := norm(k)
		entry := k + "=" + v
		if i, ok := index[nk]; ok {
			out[i] = entry
			return
		}
		index[nk] = len(out)
		out = append(out, entry)
	}

	for _, kv := range parent {
		k, v, ok := strings.Cut(kv, "=")
		if !ok || k == "" || v == "" {
			continue
		}
		if allowInheritedEnv(windows, k) {
			set(k, v)
		}
	}

	for _, kv := range extra {
		k, v, ok := strings.Cut(kv, "=")
		if !ok || k == "" {
			continue
		}
		set(k, v)
	}

	// On Windows, os.TempDir (GetTempPath2) falls back to C:\Windows when
	// TMP, TEMP, and USERPROFILE are all unset, causing os.CreateTemp to fail
	// with Access Denied for non-elevated users. Preserve a writable temp dir
	// without inheriting USERPROFILE.
	if windows && fallbackTemp != "" {
		_, hasTemp := index["TEMP"]
		_, hasTmp := index["TMP"]
		if !hasTemp && !hasTmp {
			set("TEMP", fallbackTemp)
			set("TMP", fallbackTemp)
		}
	}

	set("UNIC_PLUGIN_PROTOCOL", "1")
	if dir != "" {
		set("UNIC_PLUGIN_DIR", dir)
	}
	return out
}

func allowInheritedEnv(windows bool, key string) bool {
	if windows {
		switch strings.ToUpper(key) {
		case "SYSTEMROOT", "WINDIR", "TEMP", "TMP",
			"HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY":
			return true
		}
		return false
	}
	switch key {
	case "TMPDIR", "SSL_CERT_FILE", "SSL_CERT_DIR",
		"HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY",
		"http_proxy", "https_proxy", "all_proxy", "no_proxy":
		return true
	}
	return false
}
