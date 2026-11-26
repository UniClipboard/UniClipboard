package desktopui

import (
	"slices"
	"strings"
	"sync"
	"unicode"

	"github.com/mozillazg/go-pinyin"
	"golang.org/x/text/unicode/norm"

	"github.com/uniclipboard/unic/plugin"
)

var (
	pinyinArgs = func() pinyin.Args {
		a := pinyin.NewArgs()
		a.Style = pinyin.Normal
		a.Heteronym = true
		return a
	}()
	pinyinCache sync.Map // map[string][][]string
)

// foldSearchText lowercases s and strips Unicode combining diacritics (Mn)
// so Latin-script locales (e.g. pt-BR "código" -> "codigo") match comfortably.
// Used strictly for Desktop UI candidate autocomplete, never for Engine body search.
func foldSearchText(s string) string {
	s = strings.ToLower(strings.TrimSpace(s))
	if s == "" {
		return ""
	}
	decomposed := norm.NFD.String(s)
	var b strings.Builder
	b.Grow(len(decomposed))
	for _, r := range decomposed {
		if unicode.Is(unicode.Mn, r) {
			continue
		}
		b.WriteRune(r)
	}
	return b.String()
}

func isASCIIAlnum(s string) bool {
	if s == "" {
		return false
	}
	for i := 0; i < len(s); i++ {
		c := s[i]
		if !((c >= 'a' && c <= 'z') || (c >= '0' && c <= '9')) {
			return false
		}
	}
	return true
}

func hasHanRune(s string) bool {
	for _, r := range s {
		if unicode.Is(unicode.Han, r) {
			return true
		}
	}
	return false
}

// pinyinSegments splits s into phonetic segments: each Han rune produces its
// deduplicated Mandarin pinyin readings (heteronyms included), and contiguous
// ASCII alphanumeric runs become single lowercased segments.
func pinyinSegments(s string) [][]string {
	if !hasHanRune(s) {
		return nil
	}
	if cached, ok := pinyinCache.Load(s); ok {
		return cached.([][]string)
	}
	var segs [][]string
	var ascii strings.Builder
	flushASCII := func() {
		if ascii.Len() > 0 {
			segs = append(segs, []string{ascii.String()})
			ascii.Reset()
		}
	}
	for _, r := range s {
		if len(segs) >= 32 {
			break
		}
		switch {
		case unicode.Is(unicode.Han, r):
			flushASCII()
			raw := pinyin.SinglePinyin(r, pinyinArgs)
			if len(raw) == 0 {
				continue
			}
			var uniq []string
			for _, syl := range raw {
				syl = strings.ToLower(syl)
				if syl != "" && !slices.Contains(uniq, syl) {
					uniq = append(uniq, syl)
				}
			}
			if len(uniq) > 0 {
				segs = append(segs, uniq)
			}
		case (r >= 'a' && r <= 'z') || (r >= 'A' && r <= 'Z') || (r >= '0' && r <= '9'):
			ascii.WriteRune(unicode.ToLower(r))
		default:
			flushASCII()
		}
	}
	flushASCII()
	pinyinCache.Store(s, segs)
	return segs
}

// matchPinyinSegments checks whether an ASCII needle matches Han/mixed segments
// via full pinyin, initials, mixed full+initial (e.g. "wenj"), or partial syllable prefix.
// Returns (rank, exact, matched):
//   - exact=true when needle consumes all segments from 0 via initials or full syllables
//   - rank: 2 (exact from start), 4 (prefix from start), 6 (match starting at a later segment)
func matchPinyinSegments(segs [][]string, needle string) (int, bool) {
	if len(segs) == 0 || len(needle) == 0 || len(needle) > 48 || !isASCIIAlnum(needle) {
		return 0, false
	}
	// Check exact consumption of all segments starting at 0 (full syllables or initials).
	if matchPinyinExact(segs, 0, needle) {
		return 2, true
	}
	// Check prefix consumption starting at segment 0.
	if matchPinyinPrefix(segs, 0, needle) {
		return 4, true
	}
	// Check match starting at any later segment (e.g. "wd" in "项目文档", "gzz" in "Linux 工作站").
	for start := 1; start < len(segs); start++ {
		if matchPinyinPrefix(segs, start, needle) {
			return 6, true
		}
	}
	return 0, false
}

func matchPinyinExact(segs [][]string, idx int, rem string) bool {
	if rem == "" {
		return idx == len(segs)
	}
	if idx >= len(segs) {
		return false
	}
	for _, syl := range segs[idx] {
		if strings.HasPrefix(rem, syl) && matchPinyinExact(segs, idx+1, rem[len(syl):]) {
			return true
		}
		if len(syl) > 0 && rem[0] == syl[0] && matchPinyinExact(segs, idx+1, rem[1:]) {
			return true
		}
	}
	return false
}

func matchPinyinPrefix(segs [][]string, idx int, rem string) bool {
	if rem == "" {
		return true
	}
	if idx >= len(segs) {
		return false
	}
	for _, syl := range segs[idx] {
		// Remaining needle finishes inside this syllable (e.g. "wen" or "we" or "w").
		if strings.HasPrefix(syl, rem) {
			return true
		}
		// Consume whole syllable and continue.
		if strings.HasPrefix(rem, syl) && matchPinyinPrefix(segs, idx+1, rem[len(syl):]) {
			return true
		}
		// Consume initial character and continue.
		if len(syl) > 0 && rem[0] == syl[0] && matchPinyinPrefix(segs, idx+1, rem[1:]) {
			return true
		}
	}
	return false
}

// canonicalCandidateValue strips internal namespaces (such as "builtin:") so
// typing "#b" or "#u" does not falsely match every builtin tag.
func canonicalCandidateValue(dim, value string) string {
	if dim == "tag" {
		return strings.TrimPrefix(value, "builtin:")
	}
	if dim == "extension" {
		return strings.TrimPrefix(value, ".")
	}
	return value
}

var candidateAliases = map[string][]string{
	// Types
	"type:text":     {"text", "txt", "plain", "文本", "wenben", "wb", "テキスト", "tekisuto", "texto", "текст"},
	"type:richtext": {"richtext", "rich", "html", "rtf", "富文本", "fuwenben", "fwb", "リッチテキスト", "texto rico", "форматированный"},
	"type:image":    {"image", "img", "pic", "picture", "photo", "图片", "tupian", "tp", "画像", "gazou", "imagem", "изображение"},
	"type:file":     {"file", "files", "文件", "wenjian", "wj", "ファイル", "fairu", "arquivo", "arquivos", "файл", "файлы"},
	"type:folder":   {"folder", "dir", "directory", "文件夹", "目录", "wenjianjia", "wjj", "mulu", "ml", "フォルダ", "foruda", "pasta", "папка"},

	// Favorites (virtual dimension matched under "#" prefix)
	"favorite:" + plugin.TagFavorite: {
		"favorite", "favorites", "fav", "star", "starred", "pin", "pinned",
		"收藏", "已收藏", "我的收藏", "shoucang", "sc", "ysc",
		"お気に入り", "okiniiri", "favorito", "favoritos", "избранное",
	},

	// Built-in tags
	"tag:" + plugin.TagText:    {"text", "txt", "plain", "文本", "wenben", "wb", "テキスト", "texto", "текст"},
	"tag:" + plugin.TagImage:   {"image", "img", "pic", "picture", "photo", "图片", "tupian", "tp", "画像", "gazou", "imagem", "изображение"},
	"tag:" + plugin.TagFiles:   {"file", "files", "文件", "wenjian", "wj", "ファイル", "fairu", "arquivo", "arquivos", "файл", "файлы"},
	"tag:" + plugin.TagFolder:  {"folder", "dir", "directory", "文件夹", "目录", "wenjianjia", "wjj", "mulu", "ml", "フォルダ", "foruda", "pasta", "папка"},
	"tag:" + plugin.TagCode:    {"code", "src", "dev", "代码", "daima", "dm", "コード", "kodo", "codigo", "код"},
	"tag:" + plugin.TagLink:    {"link", "url", "http", "web", "链接", "网址", "lianjie", "lj", "wangzhi", "wz", "リンク", "rinku", "ссылка"},
	"tag:" + plugin.TagEmail:   {"email", "mail", "邮箱", "邮箱地址", "youxiang", "yx", "メール", "meru", "почта"},
	"tag:" + plugin.TagColor:   {"color", "colour", "hex", "rgb", "颜色", "yanse", "ys", "色", "iro", "cor", "цвет"},
	"tag:" + plugin.TagVersion: {"version", "ver", "semver", "版本号", "版本", "banben", "bbh", "バージョン", "versao", "версия"},
	"tag:" + plugin.TagUUID:    {"uuid", "guid"},
	"tag:" + plugin.TagIPv4:    {"ipv4", "ip", "addr", "address", "ip地址"},

	// Time presets
	"time:today":                {"today", "td", "今天", "今日", "jintian", "jt", "kyou", "hoje", "сегодня"},
	"time:yesterday":            {"yesterday", "yd", "昨天", "昨日", "zuotian", "zt", "kinou", "ontem", "вчера"},
	"time:day_before_yesterday": {"day_before_yesterday", "前天", "qiantian", "qt", "ototoi"},
	"time:last_7d":              {"last_7d", "7d", "7days", "week", "最近7天", "近7天", "7天", "最近 7 天", "zuijin7tian", "zj7t"},
	"time:last_30d":             {"last_30d", "30d", "30days", "month", "最近30天", "近30天", "30天", "最近 30 天", "zuijin30tian", "zj30t"},
	"time:this_week":            {"this_week", "week", "wk", "本周", "benzhou", "bz", "今週", "konshuu", "esta semana", "эта неделя"},
	"time:last_week":            {"last_week", "上周", "shangzhou", "sz", "先週", "senshuu", "semana passada", "прошлая неделя"},
	"time:this_month":           {"this_month", "month", "mo", "本月", "benyue", "by", "今月", "kongetsu", "este mes", "этот месяц"},
	"time:last_month":           {"last_month", "上月", "shangyue", "sy", "先月", "sengetsu", "mes passado", "прошлый месяц"},

	// Source special
	"source:local":     {"local", "this", "me", "本机", "本地", "benji", "bj", "bendi", "bd", "このデバイス", "este dispositivo", "это устройство"},
	"source:external":  {"external", "ext", "外部", "外部渠道", "waibu", "wb"},
	"source:migration": {"migration", "import", "imported", "迁移", "导入", "qianyi", "qy"},
}

// matchSearchCandidate evaluates how well a candidate matches partial input.
// Localized displayLabel is the translated display label in the active UI language;
// rawLabel is the canonical/source label (e.g. Chinese source key).
// Returns (rank, matched) where lower rank is a stronger match.
func matchSearchCandidate(dim, value, displayLabel, rawLabel, partial string) (int, bool) {
	needle := foldSearchText(partial)
	if needle == "" {
		return 10, true
	}

	best := 999
	consider := func(rank int) {
		if rank < best {
			best = rank
		}
	}

	canon := canonicalCandidateValue(dim, value)
	displayFold := foldSearchText(displayLabel)
	rawFold := foldSearchText(rawLabel)
	canonFold := foldSearchText(canon)

	// 1. Exact match on display label or canonical value.
	if displayFold == needle || canonFold == needle || (rawFold != "" && rawFold == needle) {
		consider(0)
	}
	// 2. Prefix match on localized display label.
	if displayFold != "" && strings.HasPrefix(displayFold, needle) {
		consider(1)
	}
	// 3. Prefix match on canonical value or raw label.
	if canonFold != "" && strings.HasPrefix(canonFold, needle) {
		consider(2)
	}
	if rawFold != "" && strings.HasPrefix(rawFold, needle) {
		consider(2)
	}
	// 4. Built-in multilingual aliases.
	for _, alias := range candidateAliases[dim+":"+value] {
		af := foldSearchText(alias)
		if af == "" {
			continue
		}
		switch {
		case af == needle:
			consider(2)
		case strings.HasPrefix(af, needle):
			consider(3)
		case len(needle) >= 2 && strings.Contains(af, needle):
			consider(6)
		}
	}
	// 5. Dynamic Chinese pinyin matching on localized display label and raw label
	// (covers both built-in Chinese labels and user-created Chinese tags / device names).
	if isASCIIAlnum(needle) {
		if r, ok := matchPinyinSegments(pinyinSegments(displayLabel), needle); ok {
			consider(r)
		}
		if rawLabel != "" && rawLabel != displayLabel {
			if r, ok := matchPinyinSegments(pinyinSegments(rawLabel), needle); ok {
				consider(r)
			}
		}
	}
	// 6. Substring match on display label, canonical value, or raw label.
	if displayFold != "" && strings.Contains(displayFold, needle) {
		consider(5)
	}
	if canonFold != "" && strings.Contains(canonFold, needle) {
		consider(5)
	}
	if rawFold != "" && strings.Contains(rawFold, needle) {
		consider(5)
	}

	if best == 999 {
		return 0, false
	}
	return best, true
}

// isExactCandidateCommit checks whether a space-terminated (pasted or typed)
// token should automatically convert into a filter chip. Kept intentionally
// conservative so partial/ambiguous text stays literal.
func isExactCandidateCommit(dim, value, displayLabel, rawLabel, partial string) bool {
	needle := foldSearchText(partial)
	if needle == "" {
		return false
	}
	canon := foldSearchText(canonicalCandidateValue(dim, value))
	if needle == canon || needle == foldSearchText(displayLabel) || (rawLabel != "" && needle == foldSearchText(rawLabel)) {
		return true
	}
	// Allow exact match on primary aliases for favorite/type/time (e.g. #收藏, #favorite, /file).
	if dim == "favorite" {
		for _, alias := range []string{"favorite", "favorites", "收藏", "已收藏"} {
			if needle == foldSearchText(alias) {
				return true
			}
		}
	}
	return false
}
