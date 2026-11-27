// Package detector identifies sensitive clipboard content without retaining or
// echoing clipboard payloads in errors, logs, or match descriptions.
package builtinsecurity

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"path"
	"regexp"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/sourceapp"
)

// Category identifies the generic class of a match without leaking payload text.
type Category string

const (
	CategoryPasswordManager  Category = "password_manager"
	CategoryBlockedApp       Category = "blocked_app"
	CategoryPrivateKey       Category = "private_key"
	CategoryToken            Category = "token"
	CategoryConnectionString Category = "connection_string"
	CategoryCredential       Category = "credential"
	CategorySensitiveFile    Category = "sensitive_file"
	CategoryCustomPattern    Category = "custom_pattern"
	CategoryCustomKeyword    Category = "custom_keyword"
)

// Match reports why an item was classified as sensitive. Neither Category nor
// Rule ever includes clipboard text, filenames, or user patterns.
type Match struct {
	Category Category
	Rule     string
}

// Options controls which built-in and user-defined rules are active.
type detectorOptions struct {
	DetectPrivateKeys       bool
	DetectCredentials       bool
	DetectConnectionStrings bool
	DetectTokensAndJWT      bool
	DetectPasswordManagers  bool
	DetectSensitiveFiles    bool
	MaxScanBytes            int
	BlockedApps             []string
	AllowedApps             []string
	CustomPatterns          []string
	CustomKeywords          []string
}

const (
	DefaultMaxScanBytes = 64 << 10
	MinMaxScanBytes     = 1 << 10
	MaxMaxScanBytes     = 1 << 20
	MaxCustomPatterns   = 32
	MaxCustomPatternLen = 512
	MaxCustomKeywords   = 64
	MaxCustomKeywordLen = 128
	MaxAppEntries       = 64
	MaxAppEntryLen      = 256
)

var (
	ErrInvalidOption = errors.New("builtin.security: invalid option")
)

// Detector evaluates clipboard items against compiled security rules. It is
// immutable and safe for concurrent use after construction.
type Detector struct {
	opts           detectorOptions
	allowedApps    map[string]struct{}
	blockedApps    map[string]struct{}
	customPatterns []*regexp.Regexp
	customKeywords []string // lowercased
}

// New validates options and compiles custom regular expressions.
func newDetector(opts detectorOptions) (*Detector, error) {
	if opts.MaxScanBytes == 0 {
		opts.MaxScanBytes = DefaultMaxScanBytes
	}
	if opts.MaxScanBytes < MinMaxScanBytes || opts.MaxScanBytes > MaxMaxScanBytes {
		return nil, ErrInvalidOption
	}
	if len(opts.BlockedApps) > MaxAppEntries || len(opts.AllowedApps) > MaxAppEntries ||
		len(opts.CustomPatterns) > MaxCustomPatterns || len(opts.CustomKeywords) > MaxCustomKeywords {
		return nil, ErrInvalidOption
	}

	allowed, err := normalizeAppSet(opts.AllowedApps)
	if err != nil {
		return nil, err
	}
	blocked, err := normalizeAppSet(opts.BlockedApps)
	if err != nil {
		return nil, err
	}

	patterns := make([]*regexp.Regexp, 0, len(opts.CustomPatterns))
	for _, raw := range opts.CustomPatterns {
		if raw == "" || len(raw) > MaxCustomPatternLen || !utf8.ValidString(raw) || strings.TrimSpace(raw) == "" {
			return nil, ErrInvalidOption
		}
		re, err := regexp.Compile(raw)
		if err != nil {
			return nil, ErrInvalidOption
		}
		patterns = append(patterns, re)
	}

	keywords := make([]string, 0, len(opts.CustomKeywords))
	for _, raw := range opts.CustomKeywords {
		trimmed := strings.TrimSpace(raw)
		if trimmed == "" || len(raw) > MaxCustomKeywordLen || !utf8.ValidString(raw) || strings.ContainsFunc(raw, unicode.IsControl) {
			return nil, ErrInvalidOption
		}
		keywords = append(keywords, strings.ToLower(trimmed))
	}

	return &Detector{
		opts:           opts,
		allowedApps:    allowed,
		blockedApps:    blocked,
		customPatterns: patterns,
		customKeywords: keywords,
	}, nil
}

func normalizeAppSet(entries []string) (map[string]struct{}, error) {
	if len(entries) == 0 {
		return nil, nil
	}
	out := make(map[string]struct{}, len(entries))
	for _, raw := range entries {
		trimmed := strings.TrimSpace(raw)
		if trimmed == "" || len(raw) > MaxAppEntryLen || !utf8.ValidString(raw) || strings.ContainsFunc(raw, unicode.IsControl) {
			return nil, ErrInvalidOption
		}
		out[normalizeAppToken(trimmed)] = struct{}{}
	}
	return out, nil
}

func normalizeAppToken(s string) string {
	s = strings.ToLower(strings.TrimSpace(s))
	s = strings.TrimSuffix(s, ".exe")
	s = strings.TrimSuffix(s, ".app")
	return s
}

func matchesAppSet(app *sourceapp.Application, set map[string]struct{}) bool {
	if app == nil || len(set) == 0 {
		return false
	}
	if app.Identifier != "" {
		if _, ok := set[normalizeAppToken(app.Identifier)]; ok {
			return true
		}
	}
	if app.Name != "" {
		if _, ok := set[normalizeAppToken(app.Name)]; ok {
			return true
		}
	}
	return false
}

// Evaluate checks item against all enabled rules in priority order.
// Returns (match, true, nil) if sensitive, (zero, false, nil) if clean,
// or an error if ctx is cancelled.
func (d *Detector) Evaluate(ctx context.Context, item plugin.Item) (Match, bool, error) {
	if err := ctx.Err(); err != nil {
		return Match{}, false, err
	}

	// 1. Explicit allowlist bypasses all rules for trusted apps.
	if matchesAppSet(item.SourceApplication, d.allowedApps) {
		return Match{}, false, nil
	}

	// 2. Explicit user-configured blocked apps.
	if matchesAppSet(item.SourceApplication, d.blockedApps) {
		return Match{Category: CategoryBlockedApp, Rule: "blocked_app"}, true, nil
	}

	// 3. Built-in password manager detection (covers text, images, files).
	if d.opts.DetectPasswordManagers && isPasswordManager(item.SourceApplication) {
		return Match{Category: CategoryPasswordManager, Rule: "password_manager"}, true, nil
	}

	// 4. Sensitive filenames for file/directory clipboard items.
	if d.opts.DetectSensitiveFiles && len(item.Files) > 0 {
		for _, f := range item.Files {
			if rule, ok := matchSensitiveFilename(f.Name); ok {
				return Match{Category: CategorySensitiveFile, Rule: rule}, true, nil
			}
		}
	}

	if item.Text == "" {
		return Match{}, false, nil
	}
	if err := ctx.Err(); err != nil {
		return Match{}, false, err
	}

	text := truncateUTF8(item.Text, d.opts.MaxScanBytes)

	// 5. Private keys (PEM, OpenSSH, PGP, PuTTY).
	if d.opts.DetectPrivateKeys {
		if rule, ok := matchPrivateKey(text); ok {
			return Match{Category: CategoryPrivateKey, Rule: rule}, true, nil
		}
	}

	// 6. Known vendor API tokens, auth headers, and verified JWTs.
	if d.opts.DetectTokensAndJWT {
		if rule, ok := matchTokenOrJWT(text); ok {
			return Match{Category: CategoryToken, Rule: rule}, true, nil
		}
	}

	if err := ctx.Err(); err != nil {
		return Match{}, false, err
	}

	// 7. Connection strings with embedded non-placeholder passwords.
	if d.opts.DetectConnectionStrings {
		if rule, ok := matchConnectionString(text); ok {
			return Match{Category: CategoryConnectionString, Rule: rule}, true, nil
		}
	}

	// 8. Structured password/secret/api_key assignments.
	if d.opts.DetectCredentials {
		if rule, ok := matchCredentialKV(text); ok {
			return Match{Category: CategoryCredential, Rule: rule}, true, nil
		}
	}

	if err := ctx.Err(); err != nil {
		return Match{}, false, err
	}

	// 9. User-defined regular expressions.
	for _, re := range d.customPatterns {
		if err := ctx.Err(); err != nil {
			return Match{}, false, err
		}
		if re.MatchString(text) {
			return Match{Category: CategoryCustomPattern, Rule: "custom_pattern"}, true, nil
		}
	}

	// 10. User-defined case-insensitive literal keywords.
	if len(d.customKeywords) > 0 {
		lower := strings.ToLower(text)
		for _, kw := range d.customKeywords {
			if strings.Contains(lower, kw) {
				return Match{Category: CategoryCustomKeyword, Rule: "custom_keyword"}, true, nil
			}
		}
	}

	return Match{}, false, nil
}

func truncateUTF8(s string, maxBytes int) string {
	if len(s) <= maxBytes {
		return s
	}
	n := maxBytes
	for n > 0 && !utf8.RuneStart(s[n]) {
		n--
	}
	return s[:n]
}

// Known password manager identifiers (lowercased, without .exe/.app suffix).
var passwordManagerIdentifiers = map[string]struct{}{
	// 1Password
	"com.1password.1password":         {},
	"com.1password.safari":            {},
	"com.agilebits.onepassword7":      {},
	"com.agilebits.onepassword-osx":   {},
	"com.agilebits.onepassword4":      {},
	"com.agilebits.onepassword-ios":   {},
	"com.onepassword.android":         {},
	"1password":                       {},
	// Bitwarden / Vaultwarden
	"com.bitwarden.desktop":           {},
	"com.x8bit.bitwarden":             {},
	"bitwarden":                       {},
	"vaultwarden":                     {},
	// KeePass family
	"org.keepassxc.keepassxc":         {},
	"keepassxc":                       {},
	"keepass":                         {},
	"keepass2":                        {},
	"com.hicknhacksoftware.macpass":   {},
	"macpass":                         {},
	"com.strongboxsafe.strongbox":     {},
	"com.strongboxsafe.strongbox.mac": {},
	"strongbox":                       {},
	"net.antelle.keeweb":              {},
	"keeweb":                          {},
	"com.kunzisoft.keepass.free":      {},
	"com.kunzisoft.keepass.pro":       {},
	"keepassdx":                       {},
	// Apple Passwords & Keychain Access
	"com.apple.passwords":             {},
	"com.apple.keychainaccess":        {},
	// LastPass
	"com.lastpass.lastpass":           {},
	"com.lastpass.lpandroid":          {},
	"lastpass":                        {},
	// Dashlane
	"com.dashlane.dashlane":           {},
	"com.dashlane.dashlanephonefinal": {},
	"dashlane":                        {},
	// Enpass
	"in.sinew.enpass-desktop":         {},
	"in.sinew.enpass":                 {},
	"enpass":                          {},
	// Proton Pass
	"me.proton.pass":                  {},
	"ch.protonmail.pass":              {},
	"proton.android.pass":             {},
	"protonpass":                      {},
	"proton-pass":                     {},
	// NordPass
	"com.nordpass.macos":              {},
	"com.nordpass.android.app.password.manager": {},
	"nordpass":                        {},
	// Keeper
	"com.callpod.keeper":              {},
	"com.callpod.android_apps.keeper": {},
	"keeperpasswordmanager":           {},
	// RoboForm
	"com.siber.roboform":              {},
	"roboform":                        {},
	// Secrets (Outer Corner)
	"com.outercorner.secrets":         {},
	"com.outercorner.secrets.ios":     {},
}

// Unambiguous display names of password managers (lowercased).
var passwordManagerNames = map[string]struct{}{
	"1password":        {},
	"bitwarden":        {},
	"keepassxc":        {},
	"keepass":          {},
	"keepassdx":        {},
	"macpass":          {},
	"strongbox":        {},
	"keeweb":           {},
	"keychain access":  {},
	"lastpass":         {},
	"dashlane":         {},
	"enpass":           {},
	"proton pass":      {},
	"nordpass":         {},
	"roboform":         {},
	"vaultwarden":      {},
	"keeper password manager": {},
}

func isPasswordManager(app *sourceapp.Application) bool {
	if app == nil {
		return false
	}
	id := normalizeAppToken(app.Identifier)
	if id != "" {
		if _, ok := passwordManagerIdentifiers[id]; ok {
			return true
		}
		// Match common bundle/WM_CLASS prefix variants like com.1password.* or org.keepassxc.*
		if strings.HasPrefix(id, "com.1password.") ||
			strings.HasPrefix(id, "com.agilebits.onepassword") ||
			strings.HasPrefix(id, "com.bitwarden.") ||
			strings.HasPrefix(id, "org.keepassxc.") {
			return true
		}
	}
	name := normalizeAppToken(app.Name)
	if name != "" {
		if _, ok := passwordManagerNames[name]; ok {
			return true
		}
	}
	return false
}

var (
	exactSensitiveFiles = map[string]string{
		"id_rsa":               "ssh_private_key",
		"id_ed25519":           "ssh_private_key",
		"id_ecdsa":             "ssh_private_key",
		"id_dsa":               "ssh_private_key",
		"id_ed25519_sk":        "ssh_private_key",
		"id_ecdsa_sk":          "ssh_private_key",
		".env":                 "env_file",
		".npmrc":               "package_credentials",
		".pypirc":              "package_credentials",
		".netrc":               "netrc_credentials",
		"_netrc":               "netrc_credentials",
		".htpasswd":            "password_file",
		".pgpass":              "database_credentials",
		".my.cnf":              "database_credentials",
		"kubeconfig":           "kubeconfig",
		"credentials.json":     "credentials_file",
		"service-account.json": "credentials_file",
		"service_account.json": "credentials_file",
		"secrets.yaml":         "secrets_file",
		"secrets.yml":          "secrets_file",
		"secrets.json":         "secrets_file",
		"secrets.toml":         "secrets_file",
		"master.key":           "master_key",
		"wallet.dat":           "wallet_file",
	}

	sensitiveFileExts = map[string]string{
		".pem":      "key_material",
		".key":      "key_material",
		".p12":      "pkcs12_bundle",
		".pfx":      "pkcs12_bundle",
		".pkcs12":   "pkcs12_bundle",
		".jks":      "keystore",
		".keystore": "keystore",
		".kdbx":     "password_vault",
		".kdb":      "password_vault",
		".ppk":      "putty_key",
		".ovpn":     "vpn_config",
		".1pux":     "password_vault",
		".opvault":  "password_vault",
	}

	safeTemplateSuffixes = []string{
		".example",
		".sample",
		".template",
		".dist",
		".defaults",
		".schema",
		".pub",
	}
)

func matchSensitiveFilename(name string) (string, bool) {
	// Normalize both slash styles to extract the basename safely.
	cleaned := strings.ReplaceAll(strings.TrimSpace(name), "\\", "/")
	base := strings.ToLower(path.Base(cleaned))
	if base == "" || base == "." || base == "/" {
		return "", false
	}

	for _, suffix := range safeTemplateSuffixes {
		if strings.HasSuffix(base, suffix) {
			return "", false
		}
	}

	if rule, ok := exactSensitiveFiles[base]; ok {
		return rule, true
	}

	if strings.HasPrefix(base, ".env.") {
		return "env_file", true
	}

	if ext := path.Ext(base); ext != "" {
		if rule, ok := sensitiveFileExts[ext]; ok {
			return rule, true
		}
	}

	return "", false
}

var (
	rePEMPrivateKey = regexp.MustCompile(`-----BEGIN (?:[A-Z0-9 ]+ )?PRIVATE KEY-----`)
	rePGPPrivateKey = regexp.MustCompile(`-----BEGIN PGP PRIVATE KEY BLOCK-----`)
	rePuTTYKey      = regexp.MustCompile(`PuTTY-User-Key-File-[23]:`)
)

func matchPrivateKey(text string) (string, bool) {
	if strings.Contains(text, "PRIVATE KEY") {
		if rePEMPrivateKey.MatchString(text) {
			return "pem_private_key", true
		}
		if rePGPPrivateKey.MatchString(text) {
			return "pgp_private_key", true
		}
	}
	if strings.Contains(text, "PuTTY-User-Key-File-") && rePuTTYKey.MatchString(text) {
		return "putty_private_key", true
	}
	return "", false
}

type namedPattern struct {
	name string
	hint string // case-sensitive fast substring check; empty means always run
	re   *regexp.Regexp
}

var tokenPatterns = []namedPattern{
	{"aws_access_key", "A", regexp.MustCompile(`\b(?:AKIA|ASIA|ABIA|ACCA)[0-9A-Z]{16}\b`)},
	{"github_token", "_", regexp.MustCompile(`\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{36,255}\b`)},
	{"github_pat", "github_pat_", regexp.MustCompile(`\bgithub_pat_[A-Za-z0-9_]{22,255}\b`)},
	{"gitlab_token", "gl", regexp.MustCompile(`\bgl(?:pat|dt)-[A-Za-z0-9_\-]{20,}\b`)},
	{"anthropic_api_key", "sk-ant-", regexp.MustCompile(`\bsk-ant-(?:api\d{2}|admin\d{2})-[A-Za-z0-9_\-]{20,}\b`)},
	{"openai_api_key", "sk-", regexp.MustCompile(`\bsk-(?:(?:proj|svcacct|admin)-[A-Za-z0-9_\-]{20,}|[A-Za-z0-9]{20,}T3BlbkFJ[A-Za-z0-9]{20,}|[A-Za-z0-9_\-]{32,})\b`)},
	{"stripe_secret_key", "_live_", regexp.MustCompile(`\b(?:sk|rk)_live_[0-9A-Za-z]{20,}\b`)},
	{"stripe_webhook_secret", "whsec_", regexp.MustCompile(`\bwhsec_[0-9A-Za-z]{24,}\b`)},
	{"slack_token", "xox", regexp.MustCompile(`\bxox[bparsc]-[0-9A-Za-z\-]{10,}\b`)},
	{"slack_webhook", "hooks.slack.com/services/", regexp.MustCompile(`https://hooks\.slack\.com/services/T[A-Z0-9]{8,}/B[A-Z0-9]{8,}/[A-Za-z0-9]{16,}`)},
	{"google_api_key", "AIza", regexp.MustCompile(`\bAIza[0-9A-Za-z_\-]{35}\b`)},
	{"npm_token", "npm_", regexp.MustCompile(`\bnpm_[A-Za-z0-9]{36}\b`)},
	{"pypi_token", "pypi-AgEI", regexp.MustCompile(`\bpypi-AgEIcHlwaS5vcmc[A-Za-z0-9_\-]{50,}\b`)},
	{"docker_pat", "dckr_pat_", regexp.MustCompile(`\bdckr_pat_[A-Za-z0-9_\-]{20,}\b`)},
	{"huggingface_token", "hf_", regexp.MustCompile(`\bhf_[A-Za-z0-9]{30,}\b`)},
	{"groq_api_key", "gsk_", regexp.MustCompile(`\bgsk_[A-Za-z0-9]{48,}\b`)},
	{"xai_api_key", "xai-", regexp.MustCompile(`\bxai-[A-Za-z0-9]{40,}\b`)},
	{"sendgrid_api_key", "SG.", regexp.MustCompile(`\bSG\.[A-Za-z0-9_\-]{22}\.[A-Za-z0-9_\-]{43}\b`)},
	{"digitalocean_token", "dop_v1_", regexp.MustCompile(`\bdop_v1_[a-f0-9]{64}\b`)},
	{"tailscale_key", "tskey-", regexp.MustCompile(`\btskey-(?:auth|api|client)-[A-Za-z0-9\-]{16,}\b`)},
	{"vault_token", "hvs.", regexp.MustCompile(`\bhvs\.[A-Za-z0-9_\-]{24,}\b`)},
	{"alibaba_access_key", "LTAI", regexp.MustCompile(`\bLTAI[A-Za-z0-9]{12,24}\b`)},
	{"tencent_secret_id", "AKID", regexp.MustCompile(`\bAKID[A-Za-z0-9]{32}\b`)},
	{"auth_header", ":", regexp.MustCompile(`(?i)\bAuthorization\s*:\s*(?:Bearer|Basic|Token)\s+[A-Za-z0-9\-._~+/]{16,}=*`)},
}

var reJWT = regexp.MustCompile(`\beyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{16,}\b`)

func matchTokenOrJWT(text string) (string, bool) {
	for _, p := range tokenPatterns {
		if p.hint != "" && !strings.Contains(text, p.hint) {
			continue
		}
		if p.re.MatchString(text) {
			return p.name, true
		}
	}
	if strings.Contains(text, "eyJ") {
		if matches := reJWT.FindAllString(text, 4); len(matches) > 0 {
			for _, candidate := range matches {
				if isValidJWTHeader(candidate) {
					return "jwt", true
				}
			}
		}
	}
	return "", false
}

func isValidJWTHeader(token string) bool {
	dot := strings.IndexByte(token, '.')
	if dot <= 0 {
		return false
	}
	decoded, err := base64.RawURLEncoding.DecodeString(token[:dot])
	if err != nil || len(decoded) == 0 || len(decoded) > 4096 {
		return false
	}
	var hdr map[string]any
	if err := json.Unmarshal(decoded, &hdr); err != nil {
		return false
	}
	_, hasAlg := hdr["alg"]
	_, hasTyp := hdr["typ"]
	return hasAlg || hasTyp
}

var reConnString = regexp.MustCompile(
	`(?i)\b(?:postgres(?:ql)?|mysql|mariadb|mongodb(?:\+srv)?|rediss?|amqps?|mssql|sqlserver|clickhouse|nats|ldaps?|s?ftps?)://[^/\s:@]*:([^/\s@]{3,})@[^\s/]+`,
)

func matchConnectionString(text string) (string, bool) {
	if !strings.Contains(text, "://") || !strings.Contains(text, "@") {
		return "", false
	}
	for _, m := range reConnString.FindAllStringSubmatch(text, 8) {
		if len(m) >= 2 && !isPlaceholderValue(m[1]) {
			return "connection_string", true
		}
	}
	return "", false
}

var reCredentialKV = regexp.MustCompile(
	`(?i)(?:password|passwd|secret|api[_-]?key|apikey|access[_-]?token|auth[_-]?token|private[_-]?key|client[_-]?secret|signing[_-]?key|encryption[_-]?key|webhook[_-]?secret|db[_-]?pass(?:word)?|master[_-]?key|bearer[_-]?token|aws_secret_access_key)[A-Z0-9_.-]*["']?\s*[:=]\s*["']?([^\s"'` + "`" + `,;}\]]{6,})["']?`,
)

func matchCredentialKV(text string) (string, bool) {
	if !strings.ContainsAny(text, ":=") {
		return "", false
	}
	for _, m := range reCredentialKV.FindAllStringSubmatch(text, 16) {
		if len(m) < 2 {
			continue
		}
		val := m[1]
		if !isPlaceholderValue(val) && !looksLikeCodeExpr(val) {
			return "credential_assignment", true
		}
	}
	return "", false
}

var placeholderExact = map[string]struct{}{
	"true":         {},
	"false":        {},
	"null":         {},
	"none":         {},
	"nil":          {},
	"undefined":    {},
	"string":       {},
	"boolean":      {},
	"number":       {},
	"integer":      {},
	"object":       {},
	"array":        {},
	"required":     {},
	"optional":     {},
	"password":     {},
	"passwd":       {},
	"secret":       {},
	"token":        {},
	"apikey":       {},
	"api_key":      {},
	"changeme":     {},
	"change_me":    {},
	"change-me":    {},
	"replace_me":   {},
	"replace-me":   {},
	"placeholder":  {},
	"example":      {},
	"sample":       {},
	"dummy":        {},
	"redacted":     {},
	"masked":       {},
	"hidden":       {},
	"todo":         {},
	"fixme":        {},
	"empty":        {},
}

func isPlaceholderValue(raw string) bool {
	s := strings.TrimSpace(raw)
	if s == "" {
		return true
	}
	// Environment variable references or templates like $PASS, ${PASS}, {{pass}}, <pass>, %PASS%
	if strings.HasPrefix(s, "$") ||
		strings.HasPrefix(s, "{{") ||
		strings.HasPrefix(s, "${") ||
		(strings.HasPrefix(s, "<") && strings.HasSuffix(s, ">")) ||
		(strings.HasPrefix(s, "%") && strings.HasSuffix(s, "%")) {
		return true
	}
	// Repeated masking characters like ******, xxxxxx, ......, ------
	if isRepeatedChar(s) {
		return true
	}
	lower := strings.ToLower(s)
	if _, ok := placeholderExact[lower]; ok {
		return true
	}
	for _, prefix := range []string{"your_", "your-", "my_", "my-", "example_", "example-", "sample_", "test_", "dummy_", "enter_"} {
		if strings.HasPrefix(lower, prefix) {
			return true
		}
	}
	return false
}

func isRepeatedChar(s string) bool {
	if len(s) == 0 {
		return true
	}
	first, size := utf8.DecodeRuneInString(s)
	if size == 0 {
		return true
	}
	for _, r := range s[size:] {
		if r != first {
			return false
		}
	}
	return true
}

func looksLikeCodeExpr(val string) bool {
	lower := strings.ToLower(val)
	for _, prefix := range []string{
		"os.getenv(",
		"os.environ",
		"process.env.",
		"env.",
		"config.",
		"cfg.",
		"req.",
		"ctx.",
		"this.",
		"self.",
	} {
		if strings.HasPrefix(lower, prefix) {
			return true
		}
	}
	return false
}
