import SwiftUI

/// Bottom toolbar replacing the TabView — search, server picker, sync.
/// Uses Liquid Glass styling on iOS 26+, falls back to thin material below.
struct HomeBottomToolbar: View {
    let serverLabel: String
    var isAutoSwitched: Bool = false
    var isSyncing: Bool = false
    /// Shared glass namespace from the host so the search button can liquid-morph
    /// into the search field's capsule when search opens. `nil` in standalone use
    /// (e.g. previews), where the search button simply renders without morphing.
    /// The host wraps this toolbar in the unified `GlassEffectContainer`, so this
    /// view no longer supplies its own `GlassGroup`.
    var glassNamespace: Namespace.ID? = nil
    let onSearch: () -> Void
    let onServerPicker: () -> Void
    let onSync: () -> Void

    var body: some View {
        HStack {
            Button(action: onSearch) {
                Image(systemName: "magnifyingglass")
                    .font(.title2)
                    .frame(width: 52, height: 52)
                    .liquidGlassCircle(interactive: true)
                    .glassMorphID("search", in: glassNamespace)
            }
            .accessibilityLabel(Text("搜索"))

            Spacer()

            Button(action: onServerPicker) {
                HStack(spacing: 6) {
                    Image(systemName: "clock.arrow.circlepath")
                    Text(serverLabel)
                        .font(.subheadline.weight(.medium))
                    Image(systemName: "chevron.up.chevron.down")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
                .frame(height: 52)
                .padding(.horizontal, 16)
                .liquidGlassCapsule(interactive: true)
                .overlay(alignment: .topTrailing) {
                    if isAutoSwitched {
                        Text("自动")
                            .font(.caption2.bold())
                            .foregroundStyle(Color.accentColor)
                            .padding(.horizontal, 5)
                            .padding(.vertical, 1)
                            .background(Color.accentColor.opacity(0.12), in: Capsule())
                            .offset(x: 6, y: -8)
                    }
                }
            }
            .accessibilityHint(Text("切换服务器"))

            Spacer()

            Button(action: onSync) {
                Group {
                    if isSyncing {
                        ProgressView()
                            .controlSize(.small)
                    } else {
                        Image(systemName: "arrow.triangle.2.circlepath")
                            .font(.title2)
                    }
                }
                .frame(width: 52, height: 52)
                .liquidGlassCircle(interactive: true)
            }
            .disabled(isSyncing)
            .accessibilityLabel(Text("立即同步"))
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }
}

#Preview {
    VStack {
        Spacer()
        // Standalone preview supplies its own container; in the app the host
        // (HomeView.bottomBar) wraps this in the unified GlassEffectContainer.
        GlassGroup {
            HomeBottomToolbar(
                serverLabel: "My Server",
                isAutoSwitched: true,
                isSyncing: false,
                onSearch: {},
                onServerPicker: {},
                onSync: {}
            )
        }
    }
}
