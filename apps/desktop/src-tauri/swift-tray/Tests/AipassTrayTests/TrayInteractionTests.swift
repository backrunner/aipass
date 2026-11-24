import AppKit
import SwiftUI
import XCTest
@testable import AipassTray

@MainActor
final class TrayInteractionTests: XCTestCase {
    // Use the rendered accessibility hierarchy of an isolated AppKit window.
    // Actions are captured at the same callback used by the desktop bridge.
    func testUnlockedPanelActions() throws {
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        app.finishLaunching()

        let model = TrayViewModel()
        var status = TrayStatus.checking
        status.agentState = "unlocked"
        status.canLock = true
        status.canStartProxy = true
        status.panelUrl = "http://127.0.0.1:8788"
        status.messages = [
            "ext.openApp": "Open AIPass", "titlebar.lock": "Lock", "tray.quit": "Quit",
            "ext.refresh": "Refresh", "server.start": "Start",
            "panel.open": "Open panel", "panel.copyAddress": "Copy address", "server.stop": "Stop",
        ]
        model.apply(status: status)
        var received: [String] = []
        let view = NSHostingView(rootView: TrayPanelView(model: model) { received.append($0) }.environment(\.accessibilityEnabled, true))
        let window = NSWindow(contentRect: NSRect(origin: .zero, size: view.fittingSize),
                              styleMask: [.titled], backing: .buffered, defer: false)
        window.contentView = view
        window.orderFront(nil)
        defer { window.orderOut(nil) }
        view.layoutSubtreeIfNeeded()
        RunLoop.current.run(until: Date().addingTimeInterval(0.15))
        let elements = descendants(of: view)
        let buttons = elements.filter { $0.accessibilityRole?() == .button }
        for (label, action) in [
            ("Open panel", "panel-open-url"), ("Copy address", "panel-copy"), ("Stop", "panel-stop"),
            ("Refresh", "refresh"), ("Start", "proxy-start"), ("Open AIPass", "open"), ("Lock", "lock-vault"), ("Quit", "quit"),
        ] {
            let control = try XCTUnwrap(buttons.first { ($0.accessibilityLabel?() ?? $0.accessibilityTitle?()) == label },
                                        "Missing button \(label) in \(buttons.map { $0.accessibilityLabel?() ?? $0.accessibilityTitle?() ?? "?" })")
            let frame = try XCTUnwrap(control.accessibilityFrame?())
            XCTAssertGreaterThan(frame.width, 0)
            XCTAssertTrue(window.frame.contains(frame), label)
            XCTAssertTrue(control.accessibilityPerformPress?() == true, label)
            XCTAssertEqual(received.last, action)
        }
    }

    func testContextMenuKeepsUnavailableActionsDisabledAndRoutesSelections() throws {
        _ = NSApplication.shared
        var actions: [String] = []
        let controller = TrayPanelController(iconData: Data(), actionHandler: { actions.append($0) }, unlockHandler: { _ in })
        var status = TrayStatus.checking
        status.canStartProxy = true
        controller.update(status: status)
        let menu = controller.makeContextMenu()
        let menus = [menu] + menu.items.compactMap(\.submenu)
        for menu in menus {
            menu.update()
            for (index, item) in menu.items.enumerated() {
                guard let action = item.representedObject as? String else { continue }
                let unavailable = ["start-agent", "lock-vault", "proxy-stop", "panel-open-url", "panel-copy", "panel-stop"]
                XCTAssertEqual(item.isEnabled, !unavailable.contains(action), action)
                if item.isEnabled {
                    menu.performActionForItem(at: index)
                    XCTAssertEqual(actions.last, action)
                }
            }
        }
        XCTAssertTrue(actions.contains("proxy-start"))
        XCTAssertTrue(actions.contains("quit"))
    }

    func testLockedTrayRefreshPreservesPendingUnlockAndExternalUnlockClearsDraft() {
        let model = TrayViewModel()
        var status = TrayStatus.checking
        status.agentState = "locked"
        model.apply(status: status)
        model.unlockPassword = "fixture"
        model.isPasswordVisible = true
        model.markBusy("unlock")
        model.apply(status: status)
        XCTAssertEqual(model.busyAction, "unlock")
        XCTAssertEqual(model.unlockPassword, "fixture")
        status.agentState = "unlocked"
        model.apply(status: status)
        XCTAssertEqual(model.unlockPassword, "")
        XCTAssertFalse(model.isPasswordVisible)
    }

    private func descendants(of parent: AnyObject) -> [AnyObject] {
        let children = (parent.accessibilityChildren?() ?? []).map { $0 as AnyObject }
        return children.flatMap { [$0] + descendants(of: $0) }
    }
}
