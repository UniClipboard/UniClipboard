package uikit

// semanticTokens maps shadcn-style semantic colour names to each skin's own token
// (the same table as uniclipboard ui-kit/theme/default.ts).
var semanticTokens = func() map[SkinID]map[string]string {
	rows := []struct{ name, linux, macos, windows string }{
		{"background", "window", "window", "w11-base"},
		{"foreground", "fg", "label", "colorNeutralForeground1"},
		{"card", "card", "content", "colorNeutralBackground1"},
		{"popover", "popover", "popup-opaque", "colorNeutralBackground1"},
		{"primary", "accent", "accent", "colorBrandBackground"},
		{"primaryForeground", "accent-fg", "on-accent", "colorNeutralForegroundOnBrand"},
		{"secondary", "btn", "fill-2", "colorNeutralBackground1Hover"},
		{"muted", "btn", "fill-2", "colorNeutralBackground1Hover"},
		{"mutedForeground", "fg2", "label-2", "colorNeutralForeground2"},
		{"accent", "hover", "fill-2", "colorSubtleBackgroundHover"},
		{"destructive", "destructive", "red", "colorPaletteRedForeground1"},
		{"border", "border", "separator", "colorNeutralStroke1"},
		{"input", "border", "separator", "colorNeutralStroke1"},
		{"ring", "accent", "accent", "colorBrandBackground"},
		{"chart1", "accent", "accent", "colorBrandBackground"},
		{"chart2", "green", "green", "w11-status-positive"},
		{"chart3", "yellow", "yellow", "w11-status-warning"},
		{"chart4", "purple", "purple", "colorPalettePurpleBorderActive"},
		{"chart5", "teal", "pink", "colorPaletteTealBorderActive"},
		{"sidebar", "sidebar", "window", "w11-base"},
	}
	m := map[SkinID]map[string]string{Linux: {}, MacOS: {}, Windows: {}}
	for _, r := range rows {
		m[Linux][r.name], m[MacOS][r.name], m[Windows][r.name] = r.linux, r.macos, r.windows
	}
	return m
}()
