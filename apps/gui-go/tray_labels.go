package main

import "strings"

// trayLabels are the localized tray menu strings. Locales mirror the frontend's
// supported set; anything unknown falls back to English.
type trayLabels struct {
	open, settings, checkUpdate, restart, lightweight, quit string
	syncOn, syncOff, syncError                              string
}

var trayLabelTable = map[string]trayLabels{
	"zh-CN": {"打开", "设置", "检查更新…", "重启", "轻量模式（后台同步）", "退出", "关闭同步", "开启同步", "无法更改同步状态，请稍后重试。"},
	"zh-TW": {"開啟", "設定", "檢查更新…", "重新啟動", "輕量模式（背景同步）", "結束", "關閉同步", "開啟同步", "無法變更同步狀態，請稍後重試。"},
	"ja-JP": {"開く", "設定", "アップデートを確認…", "再起動", "軽量モード（バックグラウンド同期）", "終了", "同期をオフにする", "同期をオンにする", "同期設定を変更できませんでした。もう一度お試しください。"},
	"ru-RU": {"Открыть", "Настройки", "Проверить обновления…", "Перезапустить", "Лёгкий режим (фоновая синхронизация)", "Выйти", "Выключить синхронизацию", "Включить синхронизацию", "Не удалось изменить синхронизацию. Повторите попытку позже."},
	"pt-BR": {"Abrir", "Configurações", "Verificar atualizações…", "Reiniciar", "Modo Leve (sincronização em segundo plano)", "Sair", "Desativar sincronização", "Ativar sincronização", "Não foi possível alterar a sincronização. Tente novamente."},
	"en":    {"Open", "Settings", "Check for Updates…", "Restart", "Lightweight Mode (Background Sync)", "Quit", "Disable Sync", "Enable Sync", "Could not change sync. Please try again."},
}

// normalizeTrayLanguage maps a language tag to a supported locale, following
// the daemon-side normalization (zh with Hant/TW/HK/MO subtags is Traditional).
func normalizeTrayLanguage(tag string) string {
	subtags := strings.FieldsFunc(tag, func(r rune) bool { return r == '-' || r == '_' })
	if len(subtags) == 0 {
		return "en"
	}
	switch strings.ToLower(subtags[0]) {
	case "zh":
		for _, subtag := range subtags[1:] {
			switch strings.ToLower(subtag) {
			case "hant", "tw", "hk", "mo":
				return "zh-TW"
			}
		}
		return "zh-CN"
	case "ja":
		return "ja-JP"
	case "ru":
		return "ru-RU"
	case "pt":
		return "pt-BR"
	}
	return "en"
}
