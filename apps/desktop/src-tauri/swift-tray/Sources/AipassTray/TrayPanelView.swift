import AppKit
import SwiftUI

public final class TrayViewModel: ObservableObject {
    @Published public var status: TrayStatus = .checking
    @Published public var lastUpdated: Date?
    @Published public var busyAction: String?
    @Published public var unlockPassword: String = ""
    @Published public var unlockError: String?

    public init() {}

    public func apply(status: TrayStatus) {
        self.status = status
        lastUpdated = Date()
        busyAction = nil
    }

    func markBusy(_ action: String) {
        busyAction = action
    }

    func clearBusy() {
        busyAction = nil
    }
}

public struct TrayPanelView: View {
    @ObservedObject public var model: TrayViewModel
    public var onAction: (String) -> Void
    public var onUnlock: (String) -> Void
    @FocusState private var unlockFieldFocused: Bool

    public init(
        model: TrayViewModel,
        onAction: @escaping (String) -> Void,
        onUnlock: @escaping (String) -> Void = { _ in }
    ) {
        self.model = model
        self.onAction = onAction
        self.onUnlock = onUnlock
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            header
            if model.status.agentState == "locked" {
                unlockCard
            }
            proxyRow
            divider
            actionsSection
            divider
            footer
        }
        .padding(12)
        .frame(width: TrayMetrics.panelWidth)
        .background(TrayColors.surface)
        .clipShape(RoundedRectangle(cornerRadius: TrayMetrics.panelRadius, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: TrayMetrics.panelRadius, style: .continuous)
                .stroke(TrayColors.border, lineWidth: 1)
        )
    }

    // MARK: - Header

    private var header: some View {
        HStack(spacing: 8) {
            if let icon = NSApp.applicationIconImage {
                Image(nsImage: icon)
                    .resizable()
                    .frame(width: 20, height: 20)
                    .clipShape(RoundedRectangle(cornerRadius: 4.5, style: .continuous))
            }
            Text("AIPass")
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(TrayColors.text)
            Spacer()
            if model.busyAction != nil && model.busyAction != "unlock" {
                ProgressView()
                    .controlSize(.small)
                    .scaleEffect(0.7)
                    .frame(width: 14, height: 14)
            }
            agentPill
        }
    }

    private var agentPill: some View {
        let (label, color): (String, Color) = {
            switch model.status.agentState {
            case "unlocked": return ("Unlocked", TrayColors.success)
            case "locked": return ("Locked", TrayColors.warning)
            case "no-vault": return ("No Vault", TrayColors.textTertiary)
            case "unreachable": return ("Offline", TrayColors.danger)
            default: return ("Checking…", TrayColors.textTertiary)
            }
        }()
        return Text(label)
            .font(.system(size: 11, weight: .medium))
            .foregroundStyle(color)
            .padding(.horizontal, 8)
            .padding(.vertical, 2)
            .background(color.opacity(0.12))
            .clipShape(Capsule())
    }

    // MARK: - Unlock card

    private var unlockCard: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 7) {
                Image(systemName: "lock.fill")
                    .font(.system(size: 12))
                    .foregroundStyle(TrayColors.warning)
                Text("Vault Locked")
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(TrayColors.text)
            }
            HStack(spacing: 8) {
                SecureField("Master Password", text: $model.unlockPassword)
                    .textFieldStyle(.roundedBorder)
                    .font(.system(size: 12))
                    .focused($unlockFieldFocused)
                    .disabled(model.busyAction == "unlock")
                    .onSubmit(submitUnlock)
                Button(action: submitUnlock) {
                    if model.busyAction == "unlock" {
                        ProgressView()
                            .controlSize(.small)
                            .scaleEffect(0.7)
                            .frame(width: 14, height: 14)
                    } else {
                        Text("Unlock")
                    }
                }
                .buttonStyle(.borderedProminent)
                .tint(TrayColors.accent)
                .controlSize(.small)
                .disabled(model.unlockPassword.isEmpty || model.busyAction == "unlock")
                .keyboardShortcut(.defaultAction)
            }
            if let error = model.unlockError {
                Text(error)
                    .font(.system(size: 11))
                    .foregroundStyle(TrayColors.danger)
                    .lineLimit(2)
            }
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(TrayColors.surface2)
        .clipShape(RoundedRectangle(cornerRadius: TrayMetrics.cardRadius, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: TrayMetrics.cardRadius, style: .continuous)
                .stroke(TrayColors.border, lineWidth: 1)
        )
    }

    private func submitUnlock() {
        let password = model.unlockPassword
        guard !password.isEmpty, model.busyAction == nil || model.busyAction == "unlock" else { return }
        onUnlock(password)
    }

    // MARK: - Proxy row

    private var proxyRow: some View {
        HStack(spacing: 7) {
            Circle()
                .fill(proxyStateColor)
                .frame(width: 7, height: 7)
            Text("Proxy")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(TrayColors.text)
            Spacer(minLength: 4)
            Text(proxyStatusText)
                .font(.system(size: 11, design: .monospaced))
                .foregroundStyle(TrayColors.textTertiary)
                .lineLimit(1)
                .truncationMode(.middle)
            if model.status.canStartProxy {
                Button("Start") { onAction("proxy-start") }
                    .buttonStyle(.borderedProminent)
                    .tint(TrayColors.accent)
                    .controlSize(.small)
                    .disabled(model.busyAction != nil)
            } else if model.status.canStopProxy {
                Button("Stop") { onAction("proxy-stop") }
                    .buttonStyle(.bordered)
                    .tint(TrayColors.danger)
                    .controlSize(.small)
                    .disabled(model.busyAction != nil)
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(TrayColors.surface2)
        .clipShape(RoundedRectangle(cornerRadius: TrayMetrics.cardRadius, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: TrayMetrics.cardRadius, style: .continuous)
                .stroke(TrayColors.border, lineWidth: 1)
        )
        // Full detail (e.g. "Status: Running | 127.0.0.1:8787 | 3 routes") on hover.
        .help(model.status.proxyText)
    }

    /// One-line status: "Running · 3 routes" when up, otherwise the short state label.
    private var proxyStatusText: String {
        if model.status.proxyState == "running", let detail = model.status.proxyDetail {
            let routes = detail.components(separatedBy: "· ").last ?? detail
            return "Running · \(routes)"
        }
        return model.status.proxyStateText
    }

    private var proxyStateColor: Color {
        switch model.status.proxyState {
        case "running": return TrayColors.success
        case "stopped", "no-vault": return TrayColors.textTertiary
        case "locked": return TrayColors.warning
        case "unavailable": return TrayColors.danger
        default: return TrayColors.textTertiary
        }
    }

    // MARK: - Actions

    private var actionsSection: some View {
        VStack(alignment: .leading, spacing: 2) {
            actionRow("macwindow", title: "Open Desktop", action: "open")
            if model.status.agentState == "locked" {
                Button {
                    unlockFieldFocused = true
                } label: {
                    HStack(spacing: 9) {
                        Image(systemName: "lock.open")
                            .font(.system(size: 12))
                            .frame(width: 16, alignment: .center)
                        Text("Unlock Vault")
                            .font(.system(size: 13))
                        Spacer()
                    }
                    .foregroundStyle(TrayColors.text)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 6)
                    .contentShape(RoundedRectangle(cornerRadius: TrayMetrics.rowRadius, style: .continuous))
                }
                .buttonStyle(TrayRowButtonStyle())
            } else if model.status.canLock {
                actionRow("lock", title: "Lock Vault", action: "lock-vault")
            }
        }
    }

    private func actionRow(_ systemImage: String, title: String, action: String, enabled: Bool = true) -> some View {
        Button {
            onAction(action)
        } label: {
            HStack(spacing: 9) {
                Image(systemName: systemImage)
                    .font(.system(size: 12))
                    .frame(width: 16, alignment: .center)
                Text(title)
                    .font(.system(size: 13))
                Spacer()
            }
            .foregroundStyle(TrayColors.text)
            .padding(.horizontal, 8)
            .padding(.vertical, 6)
            .contentShape(RoundedRectangle(cornerRadius: TrayMetrics.rowRadius, style: .continuous))
        }
        .buttonStyle(TrayRowButtonStyle())
        .disabled(!enabled || model.busyAction != nil)
        .opacity(enabled ? 1 : 0.5)
    }

    // MARK: - Footer

    private var footer: some View {
        HStack(spacing: 6) {
            Button {
                onAction("refresh")
            } label: {
                Image(systemName: "arrow.clockwise")
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(TrayColors.textSecondary)
                    .padding(5)
                    .contentShape(RoundedRectangle(cornerRadius: TrayMetrics.rowRadius, style: .continuous))
            }
            .buttonStyle(TrayRowButtonStyle())
            .help("Refresh Status")

            if let lastUpdated = model.lastUpdated {
                Text("Updated at \(lastUpdated, style: .time)")
                    .font(.system(size: 11))
                    .foregroundStyle(TrayColors.textTertiary)
            } else {
                Text("Checking…")
                    .font(.system(size: 11))
                    .foregroundStyle(TrayColors.textTertiary)
            }

            Spacer()

            Button {
                onAction("quit")
            } label: {
                HStack(spacing: 5) {
                    Image(systemName: "power")
                        .font(.system(size: 11, weight: .medium))
                    Text("Quit")
                        .font(.system(size: 12, weight: .medium))
                }
                .foregroundStyle(TrayColors.danger)
                .padding(.horizontal, 8)
                .padding(.vertical, 5)
                .contentShape(RoundedRectangle(cornerRadius: TrayMetrics.rowRadius, style: .continuous))
            }
            .buttonStyle(TrayRowButtonStyle())
        }
    }

    private var divider: some View {
        TrayColors.border.frame(height: 1)
    }
}

/// Hover-highlight row style matching the desktop app's menu rows.
private struct TrayRowButtonStyle: ButtonStyle {
    @State private var hovering = false

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .background(
                RoundedRectangle(cornerRadius: TrayMetrics.rowRadius, style: .continuous)
                    .fill(hovering || configuration.isPressed ? TrayColors.surface2 : Color.clear)
            )
            .onHover { hovering = $0 }
            .animation(.easeOut(duration: 0.12), value: hovering)
    }
}
