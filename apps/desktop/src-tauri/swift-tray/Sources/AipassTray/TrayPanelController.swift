import AppKit
import SwiftUI

/// Owns the status bar item, the SwiftUI popover panel (left click) and the
/// classic fallback menu (right click). All logic stays in Rust; this layer
/// only renders the last pushed `TrayStatus` and forwards action ids.
final class TrayPanelController: NSObject, NSMenuDelegate {
    private let iconData: Data
    private let actionHandler: (String) -> Void
    private let unlockHandler: (String) -> Void

    private var statusItem: NSStatusItem?
    private var panel: NSPanel?
    private var hostingView: NSHostingView<TrayPanelView>?
    private let viewModel = TrayViewModel()
    private var status: TrayStatus?

    private var globalMonitor: Any?
    private var localMonitor: Any?

    init(iconData: Data, actionHandler: @escaping (String) -> Void, unlockHandler: @escaping (String) -> Void) {
        self.iconData = iconData
        self.actionHandler = actionHandler
        self.unlockHandler = unlockHandler
    }

    // MARK: - Lifecycle

    func install() {
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        if let button = item.button {
            let image = NSImage(data: iconData)
            image?.isTemplate = true
            image?.size = NSSize(width: 18, height: 18)
            button.image = image
            button.target = self
            button.action = #selector(statusItemClicked(_:))
            button.sendAction(on: [.leftMouseUp, .rightMouseUp])
            button.toolTip = "AIPass Agent"
        }
        statusItem = item
    }

    func uninstall() {
        closePanel()
        if let statusItem {
            NSStatusBar.system.removeStatusItem(statusItem)
        }
        statusItem = nil
    }

    func update(status: TrayStatus) {
        self.status = status
        viewModel.apply(status: status)
        statusItem?.button?.toolTip = status.tooltip
        if let panel, panel.isVisible {
            positionPanel(panel)
        }
    }

    // MARK: - Status item events

    @objc private func statusItemClicked(_ sender: NSStatusBarButton) {
        if NSApp.currentEvent?.type == .rightMouseUp {
            showContextMenu()
        } else {
            togglePanel()
        }
    }

    // MARK: - Panel

    private func togglePanel() {
        if let panel, panel.isVisible {
            closePanel()
        } else {
            showPanel()
        }
    }

    private func showPanel() {
        let panel = panel ?? makePanel()
        self.panel = panel
        actionHandler("panel-open")
        positionPanel(panel)
        panel.orderFrontRegardless()
        installDismissMonitors()
    }

    func closePanel() {
        panel?.orderOut(nil)
        removeDismissMonitors()
        viewModel.clearBusy()
    }

    private func makePanel() -> NSPanel {
        let hosting = NSHostingView(rootView: TrayPanelView(
            model: viewModel,
            onAction: { [weak self] actionId in
                self?.handleAction(actionId)
            },
            onUnlock: { [weak self] password in
                self?.handleUnlock(password)
            }
        ))
        hostingView = hosting

        let panel = TrayPanel(
            contentRect: NSRect(x: 0, y: 0, width: TrayMetrics.panelWidth, height: 320),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.isFloatingPanel = true
        panel.level = .popUpMenu
        panel.backgroundColor = .clear
        panel.isOpaque = false
        panel.hasShadow = true
        panel.hidesOnDeactivate = false
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.contentView = hosting
        return panel
    }

    private func positionPanel(_ panel: NSPanel) {
        guard let button = statusItem?.button, let buttonWindow = button.window else { return }
        hostingView?.layoutSubtreeIfNeeded()
        let size = hostingView?.fittingSize ?? NSSize(width: TrayMetrics.panelWidth, height: 320)
        let buttonFrame = buttonWindow.convertToScreen(button.convert(button.bounds, to: nil))
        var origin = NSPoint(
            x: buttonFrame.midX - size.width / 2,
            y: buttonFrame.minY - size.height - 6
        )
        if let screen = buttonWindow.screen ?? NSScreen.main {
            let frame = screen.visibleFrame
            origin.x = Swift.min(Swift.max(origin.x, frame.minX + 8), frame.maxX - size.width - 8)
        }
        panel.setFrame(NSRect(origin: origin, size: size), display: true)
    }

    private func installDismissMonitors() {
        removeDismissMonitors()
        globalMonitor = NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown]) { [weak self] _ in
            self?.closePanel()
        }
        localMonitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .keyDown]) { [weak self] event in
            guard let self else { return event }
            if event.type == .keyDown {
                if event.keyCode == 53 { // ESC
                    self.closePanel()
                    return nil
                }
                return event
            }
            if let panel = self.panel, event.window !== panel {
                self.closePanel()
            }
            return event
        }
    }

    private func removeDismissMonitors() {
        if let globalMonitor {
            NSEvent.removeMonitor(globalMonitor)
            self.globalMonitor = nil
        }
        if let localMonitor {
            NSEvent.removeMonitor(localMonitor)
            self.localMonitor = nil
        }
    }

    // MARK: - Actions

    private func handleAction(_ actionId: String) {
        switch actionId {
        case "open", "hide", "quit", "lock-vault", "proxy-open":
            closePanel()
        default:
            viewModel.markBusy(actionId)
        }
        actionHandler(actionId)
    }

    private func handleUnlock(_ password: String) {
        viewModel.markBusy("unlock")
        viewModel.unlockError = nil
        unlockHandler(password)
    }

    /// Called by the bridge with the Rust-side unlock outcome (nil = success).
    func reportUnlockResult(_ error: String?) {
        viewModel.clearBusy()
        if let error {
            viewModel.unlockError = error
        } else {
            viewModel.unlockError = nil
            viewModel.unlockPassword = ""
        }
    }

    // MARK: - Right-click fallback menu

    private func showContextMenu() {
        guard let statusItem else { return }
        let snapshot = status
        let menu = NSMenu()
        menu.delegate = self

        let agentLine = NSMenuItem(title: snapshot?.agentText ?? "Agent: checking...", action: nil, keyEquivalent: "")
        agentLine.isEnabled = false
        menu.addItem(agentLine)
        menu.addItem(.separator())
        menu.addItem(makeItem("Open AIPass", action: "open"))
        menu.addItem(makeItem("Hide Window", action: "hide"))
        menu.addItem(.separator())
        menu.addItem(makeItem("Refresh Status", action: "refresh"))
        menu.addItem(makeItem("Start Agent", action: "start-agent", enabled: snapshot?.canStartAgent ?? false))
        menu.addItem(makeItem("Lock Vault", action: "lock-vault", enabled: snapshot?.canLock ?? false))
        menu.addItem(makeItem("Repair Auto-Start", action: "repair-autostart"))

        let proxyMenu = NSMenu()
        let proxyLine = NSMenuItem(title: snapshot?.proxyText ?? "Status: checking...", action: nil, keyEquivalent: "")
        proxyLine.isEnabled = false
        proxyMenu.addItem(proxyLine)
        proxyMenu.addItem(makeItem("Open Server", action: "proxy-open", enabled: snapshot?.canOpenProxy ?? true))
        proxyMenu.addItem(.separator())
        proxyMenu.addItem(makeItem("Start Proxy", action: "proxy-start", enabled: snapshot?.canStartProxy ?? false))
        proxyMenu.addItem(makeItem("Stop Proxy", action: "proxy-stop", enabled: snapshot?.canStopProxy ?? false))
        proxyMenu.addItem(makeItem("Refresh Proxy Status", action: "refresh"))
        let proxyItem = NSMenuItem(title: "Proxy Server", action: nil, keyEquivalent: "")
        proxyItem.submenu = proxyMenu
        menu.addItem(proxyItem)

        menu.addItem(.separator())
        menu.addItem(makeItem("Quit AIPass", action: "quit"))

        statusItem.menu = menu
        statusItem.button?.performClick(nil)
    }

    private func makeItem(_ title: String, action: String, enabled: Bool = true) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: #selector(menuItemSelected(_:)), keyEquivalent: "")
        item.target = self
        item.representedObject = action
        item.isEnabled = enabled
        return item
    }

    @objc private func menuItemSelected(_ sender: NSMenuItem) {
        guard let actionId = sender.representedObject as? String else { return }
        actionHandler(actionId)
    }

    func menuDidClose(_ menu: NSMenu) {
        // Restore left-click panel behavior after the transient menu closes.
        statusItem?.menu = nil
    }
}

/// Borderless floating panel that can take key status (for ESC) without
/// activating the app.
private final class TrayPanel: NSPanel {
    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }
}
