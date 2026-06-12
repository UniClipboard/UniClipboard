#!/usr/bin/env bash
#
# sim-language.sh — switch an iOS Simulator's *system* language + locale and
# re-apply a clean, App-Store-grade status bar. For capturing system-UI
# screenshots (Settings, the share sheet, the system keyboard) in a target
# language.
#
# Why this exists: a per-launch `-AppleLanguages '(en)'` override only localizes
# OUR app. System surfaces — e.g. Settings › UniClip › "Paste from Other Apps"
# (the OnboardingPaste capture) — follow the *global* simulator language, which
# can only be changed by writing NSGlobalDomain and rebooting the device. The
# reboot also clears any prior `status_bar override`, so we re-apply it here.
#
# Usage:
#   scripts/sim-language.sh [lang]          # lang defaults to "en"
#
# Examples:
#   scripts/sim-language.sh                  # → English (en_US)
#   scripts/sim-language.sh zh-Hans          # → 简体中文 (zh_CN), the catalog source
#   DEV="iPhone 16 Pro" APPEARANCE=dark scripts/sim-language.sh en
#
# Env overrides:
#   DEV          simulator name or UDID      (default: "iPhone 17 Pro")
#   LOCALE       region code                 (default: derived from lang)
#   APPEARANCE   light | dark                (default: light)
#   NO_STATUSBAR=1   skip the status-bar override
#   NO_OPEN=1        don't open Settings.app / the Simulator GUI afterwards
#
set -euo pipefail

LANG_CODE="${1:-en}"
DEV="${DEV:-iPhone 17 Pro}"
APPEARANCE="${APPEARANCE:-light}"

# Derive a sensible region if LOCALE wasn't supplied.
if [[ -z "${LOCALE:-}" ]]; then
  case "$LANG_CODE" in
    en)      LOCALE="en_US" ;;
    zh-Hans) LOCALE="zh_CN" ;;
    zh-Hant) LOCALE="zh_TW" ;;
    ja)      LOCALE="ja_JP" ;;
    ko)      LOCALE="ko_KR" ;;
    *)       LOCALE="${LANG_CODE}_US" ;;
  esac
fi

echo "▶︎ device     : $DEV"
echo "▶︎ language   : $LANG_CODE"
echo "▶︎ locale     : $LOCALE"
echo "▶︎ appearance : $APPEARANCE"

# 1. Boot (if needed) so we can write the device's prefs.
xcrun simctl bootstatus "$DEV" -b >/dev/null

# 2. Write the GLOBAL language + locale — this is what system apps read.
xcrun simctl spawn "$DEV" defaults write -g AppleLanguages -array "$LANG_CODE"
xcrun simctl spawn "$DEV" defaults write -g AppleLocale    -string "$LOCALE"

# 3. Reboot so SpringBoard + every system app pick up the new language.
echo "↻ rebooting to apply language…"
xcrun simctl shutdown "$DEV"
xcrun simctl bootstatus "$DEV" -b >/dev/null

# 4. Appearance (reboot leaves this at the device default).
xcrun simctl ui "$DEV" appearance "$APPEARANCE" >/dev/null 2>&1 || true

# 5. Re-apply a clean status bar (the reboot wiped any prior override).
if [[ "${NO_STATUSBAR:-}" != "1" ]]; then
  xcrun simctl status_bar "$DEV" override \
    --time "9:41" \
    --batteryState charged --batteryLevel 100 \
    --cellularMode active --cellularBars 4 \
    --wifiMode active --wifiBars 3
  echo "✓ status bar → 9:41 / full battery / full signal"
fi

# 6. Surface the GUI + Settings so the manual nav is one tap away.
if [[ "${NO_OPEN:-}" != "1" ]]; then
  open -a Simulator || true
  xcrun simctl launch "$DEV" com.apple.Preferences >/dev/null 2>&1 || true
fi

echo "✓ done — system language is now '$LANG_CODE'."
echo "  Next: Settings › UniClip › Paste from Other Apps → Allow, then:"
echo "    xcrun simctl io \"$DEV\" screenshot ~/Desktop/OnboardingPaste-${APPEARANCE}.png"
echo "  (the 'Paste from Other Apps' row appears only after UniClip has"
echo "   triggered one 'Allow Paste' prompt — copy text elsewhere, open UniClip.)"
