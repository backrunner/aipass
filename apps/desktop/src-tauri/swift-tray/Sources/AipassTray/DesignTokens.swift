import AppKit
import SwiftUI

/// Design tokens translated from `packages/ui/src/styles/base.scss`.
enum TrayColors {
    static func dynamic(_ light: NSColor, _ dark: NSColor) -> Color {
        Color(nsColor: NSColor(name: nil, dynamicProvider: { appearance in
            let darkNames: [NSAppearance.Name] = [.darkAqua, .vibrantDark, .accessibilityHighContrastDarkAqua, .accessibilityHighContrastVibrantDark]
            let isDark = darkNames.contains(appearance.bestMatch(from: darkNames + [.aqua]) ?? .aqua)
            return isDark ? dark : light
        }))
    }

    private static func rgb(_ hex: UInt32, alpha: CGFloat = 1) -> NSColor {
        NSColor(
            calibratedRed: CGFloat((hex >> 16) & 0xFF) / 255,
            green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255,
            alpha: alpha
        )
    }

    static let surface = dynamic(rgb(0xFFFFFF, alpha: 0.96), rgb(0x171A22, alpha: 0.96))
    static let surface2 = dynamic(rgb(0xF5F6FA), rgb(0x20242E))
    static let text = dynamic(rgb(0x08101F), rgb(0xF4F6FC))
    static let textSecondary = dynamic(rgb(0x383F55), rgb(0xC8CEE0))
    static let textTertiary = dynamic(rgb(0x636B82), rgb(0x969EB6))
    static let border = dynamic(rgb(0xDDE1EA), rgb(0x303542))
    static let accent = dynamic(rgb(0x2563EB), rgb(0x6092FF))
    static let accentSoft = dynamic(rgb(0x2563EB, alpha: 0.10), rgb(0x6092FF, alpha: 0.16))
    static let success = dynamic(rgb(0x18794E), rgb(0x8AD8BE))
    static let danger = dynamic(rgb(0xB42318), rgb(0xF1A6A0))
    static let warning = dynamic(rgb(0xB25E09), rgb(0xE2B070))
}

enum TrayMetrics {
    static let panelWidth: CGFloat = 304
    static let panelRadius: CGFloat = 12
    static let cardRadius: CGFloat = 8
    static let rowRadius: CGFloat = 6
}
