package pairnotice

import (
	_ "embed"
	"encoding/json"
	"fmt"
)

//go:embed locales.json
var catalogue []byte

var messages = func() map[string]map[string]string {
	var result map[string]map[string]string
	if err := json.Unmarshal(catalogue, &result); err != nil {
		panic(err)
	}
	return result
}()

func Text(language, peer, info string) (title, body string) {
	translate := func(source string) string {
		if text := messages[source][language]; text != "" {
			return text
		}
		return source
	}
	title = translate("新设备")
	switch info {
	case "":
		body = fmt.Sprintf(translate("%s 请求同步剪贴板。打开 unic 批准或拒绝。"), peer)
	case "code":
		body = fmt.Sprintf(translate("%s 输入了本机配对码。打开 unic，核对指纹后批准。"), peer)
	default:
		body = fmt.Sprintf(translate("%s 由 %s 介绍，请求加入。打开 unic 批准或拒绝。"), peer, info)
	}
	return
}
