import SwiftUI

/// Reusable building blocks for half-sheet guides (keyboard setup, share
/// extension how-to, paste permission, etc.). All guide sheets share
/// these components to maintain a single visual language.

// MARK: - Header

/// Bold title left, circle-X close button right.
struct GuideSheetHeader: View {
    let title: LocalizedStringKey
    let onDismiss: () -> Void

    var body: some View {
        HStack(alignment: .top) {
            Text(title)
                .font(.title2.weight(.bold))
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 12)
            GuideSheetCloseButton(action: onDismiss)
        }
    }
}

/// Circle-X close button — large hit area, muted tint.
struct GuideSheetCloseButton: View {
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: "xmark.circle.fill")
                .font(.title)
                .symbolRenderingMode(.hierarchical)
                .foregroundStyle(.tertiary)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("关闭")
    }
}

// MARK: - Buttons

/// Full-width primary action button.
struct GuideSheetPrimaryButton: View {
    let title: LocalizedStringKey
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(.subheadline.weight(.semibold))
                .frame(maxWidth: .infinity)
                .padding(.vertical, 10)
                .foregroundStyle(Color(.systemBackground))
        }
        .buttonStyle(.borderedProminent)
    }
}

/// Full-width secondary action button — plain, tint-colored.
struct GuideSheetSecondaryButton: View {
    let title: LocalizedStringKey
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(.subheadline.weight(.medium))
                .frame(maxWidth: .infinity)
                .padding(.vertical, 6)
        }
        .buttonStyle(.plain)
        .foregroundStyle(.tint)
    }
}

// MARK: - Step Row

/// Numbered step row with an accent-colored circle. `isActive` defaults to
/// `true` (all steps highlighted); pass `false` for inactive carousel steps.
struct GuideSheetStepRow: View {
    let number: Int
    let title: LocalizedStringKey
    var isActive: Bool = true

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            Text("\(number)")
                .font(.footnote.weight(.bold).monospacedDigit())
                .foregroundStyle(Color(.systemBackground))
                .frame(width: 22, height: 22)
                .background(
                    Circle().fill(isActive
                                  ? Color.accentColor
                                  : Color.secondary.opacity(0.4))
                )
            Text(title)
                .font(.callout)
                .foregroundStyle(isActive ? .primary : .secondary)
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
        }
    }
}

// MARK: - Pulsing Dot

/// Animated dot overlay that draws the user's eye to a tap target in a
/// guide screenshot. Position it with `UnitPoint` (0,0 = top-left,
/// 1,1 = bottom-right of the visible image frame).
struct GuidePulsingDot: View {
    @State private var isPulsing = false

    var body: some View {
        ZStack {
            Circle()
                .stroke(Color.accentColor, lineWidth: 2)
                .frame(width: 28, height: 28)
                .scaleEffect(isPulsing ? 1.6 : 1)
                .opacity(isPulsing ? 0 : 0.8)
            Circle()
                .fill(Color.accentColor)
                .frame(width: 12, height: 12)
                .shadow(color: .black.opacity(0.25), radius: 3)
        }
        .onAppear {
            withAnimation(.easeOut(duration: 1.3).repeatForever(autoreverses: false)) {
                isPulsing = true
            }
        }
    }
}

// MARK: - Layout Constants

enum GuideSheetLayout {
    static let contentPadding: CGFloat = 24
    static let stepSpacing: CGFloat = 12
    /// Default detent: 2/3 only, no upward expansion.
    static let detents: Set<PresentationDetent> = [.fraction(2.0 / 3.0)]
    /// Expandable detent: 2/3 + full height, user can swipe up.
    static let expandableDetents: Set<PresentationDetent> = [.fraction(2.0 / 3.0), .large]
}
