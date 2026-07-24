import AppKit
import Foundation

/// Rust-side action callback: invoked on the main thread with an action id.
public typealias AipassTrayActionCallback = @convention(c) (UnsafePointer<CChar>?) -> Void
/// Rust-side unlock callback: invoked with the master password UTF-8 string.
public typealias AipassTrayUnlockCallback = @convention(c) (UnsafePointer<CChar>?) -> Void

final class TrayBridgeContext {
    var controller: TrayPanelController?
    var callback: AipassTrayActionCallback?
    var unlockCallback: AipassTrayUnlockCallback?
}

private let context = TrayBridgeContext()

/// Called once from the Tauri setup hook (main thread) with the tray template PNG.
@_cdecl("aipass_tray_init")
public func aipass_tray_init(
    _ iconPng: UnsafePointer<UInt8>?,
    _ iconLen: Int,
    _ callback: AipassTrayActionCallback?,
    _ unlockCallback: AipassTrayUnlockCallback?
) {
    let install = {
        let data = iconPng.map { Data(bytes: $0, count: iconLen) } ?? Data()
        context.callback = callback
        context.unlockCallback = unlockCallback
        let controller = TrayPanelController(
            iconData: data,
            actionHandler: { actionId in
                guard let callback = context.callback else { return }
                actionId.withCString { callback($0) }
            },
            unlockHandler: { password in
                guard let unlockCallback = context.unlockCallback else { return }
                password.withCString { unlockCallback($0) }
            }
        )
        context.controller = controller
        controller.install()
    }
    if Thread.isMainThread {
        install()
    } else {
        DispatchQueue.main.sync(execute: install)
    }
}

/// Called from arbitrary Rust threads with a UTF-8 JSON status snapshot.
@_cdecl("aipass_tray_update_status")
public func aipass_tray_update_status(_ json: UnsafePointer<CChar>?) {
    guard let json else { return }
    let string = String(cString: json)
    DispatchQueue.main.async {
        guard let data = string.data(using: .utf8),
              let status = try? JSONDecoder().decode(TrayStatus.self, from: data)
        else { return }
        context.controller?.update(status: status)
    }
}

/// Called from Rust with the outcome of an unlock attempt (nil error = success).
@_cdecl("aipass_tray_report_unlock_result")
public func aipass_tray_report_unlock_result(_ error: UnsafePointer<CChar>?) {
    let message = error.map { String(cString: $0) }
    DispatchQueue.main.async {
        context.controller?.reportUnlockResult(message)
    }
}

/// Called before process exit; tears down the status item.
@_cdecl("aipass_tray_shutdown")
public func aipass_tray_shutdown() {
    let teardown = {
        context.controller?.uninstall()
        context.controller = nil
    }
    if Thread.isMainThread {
        teardown()
    } else {
        DispatchQueue.main.async(execute: teardown)
    }
}
