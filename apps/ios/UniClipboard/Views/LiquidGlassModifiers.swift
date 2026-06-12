import SwiftUI

extension View {
    @ViewBuilder
    func liquidGlassCard(cornerRadius: CGFloat = 18, interactive: Bool = false) -> some View {
        let shape = RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
        if #available(iOS 26.0, *) {
            self.glassEffect(interactive ? .regular.interactive() : .regular, in: shape)
        } else {
            self
                .background(.regularMaterial, in: shape)
                .overlay(shape.strokeBorder(Color.primary.opacity(0.06)))
        }
    }

    @ViewBuilder
    func liquidGlassCapsule(interactive: Bool = false) -> some View {
        let shape = Capsule(style: .continuous)
        if #available(iOS 26.0, *) {
            self.glassEffect(interactive ? .regular.interactive() : .regular, in: shape)
        } else {
            self
                .background(.ultraThinMaterial, in: shape)
                .overlay(shape.strokeBorder(Color.primary.opacity(0.06)))
        }
    }

    @ViewBuilder
    func liquidGlassCircle(interactive: Bool = false) -> some View {
        if #available(iOS 26.0, *) {
            self.glassEffect(interactive ? .regular.interactive() : .regular, in: Circle())
        } else {
            self
                .background(.ultraThinMaterial, in: Circle())
                .overlay(Circle().strokeBorder(Color.primary.opacity(0.08)))
        }
    }

    /// Gives this glass element a morph identity within the nearest
    /// `GlassEffectContainer`. Two elements that share the same `id` in the same
    /// `namespace` liquid-morph into each other when one replaces the other in
    /// the view hierarchy (inside a `withAnimation`). No-op below iOS 26 or when
    /// no namespace is supplied (e.g. a standalone preview with no host).
    @ViewBuilder
    func glassMorphID(_ id: String, in namespace: Namespace.ID?) -> some View {
        if #available(iOS 26.0, *), let namespace {
            self.glassEffectID(id, in: namespace)
        } else {
            self
        }
    }
}

/// Groups adjacent glass controls so their morph/merge animations and
/// blur sampling are coordinated. On iOS 26+ this wraps the content in a
/// `GlassEffectContainer`; on earlier OSes it is a transparent passthrough.
struct GlassGroup<Content: View>: View {
    var spacing: CGFloat? = nil
    @ViewBuilder var content: Content

    var body: some View {
        if #available(iOS 26.0, *) {
            GlassEffectContainer(spacing: spacing) { content }
        } else {
            content
        }
    }
}
