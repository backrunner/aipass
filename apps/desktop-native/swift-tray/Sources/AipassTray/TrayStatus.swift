import Foundation

/// Status snapshot pushed from the Rust side as JSON.
/// Field names match the camelCase serde DTO in `tray.rs`.
public struct TrayStatus: Codable {
    var panelUrl: String? = nil
    var locale: String
    var messages: [String: String]

    func text(_ key: String) -> String { messages[key] ?? "" }

    func text(_ key: String, time: Date) -> String {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: locale)
        formatter.timeStyle = .short
        return text(key).replacingOccurrences(of: "{time}", with: formatter.string(from: time))
    }

    /// e.g. "Agent: running (unlocked)"
    var agentText: String
    /// checking | unlocked | locked | no-vault | unreachable
    var agentState: String
    var canStartAgent: Bool
    var canLock: Bool
    /// e.g. "Status: Running | 127.0.0.1:8787 | 3 routes"
    var proxyText: String
    /// checking | running | stopped | locked | no-vault | unavailable
    var proxyState: String
    /// Short label for the proxy card, e.g. "Running", "Vault locked".
    var proxyStateText: String
    /// e.g. "127.0.0.1:8787 · 3 routes" when running.
    var proxyDetail: String?
    var proxyGroups: [TrayGroup]
    var proxyRunning: Bool
    var canOpenProxy: Bool
    var canStartProxy: Bool
    var canStopProxy: Bool
    var tooltip: String

    static let checking = TrayStatus(
        locale: "en",
        messages: [:],
        agentText: "",
        agentState: "checking",
        canStartAgent: false,
        canLock: false,
        proxyText: "",
        proxyState: "checking",
        proxyStateText: "",
        proxyDetail: nil,
        proxyGroups: [],
        proxyRunning: false,
        canOpenProxy: true,
        canStartProxy: false,
        canStopProxy: false,
        tooltip: "AIPass Agent"
    )
}

public struct TrayGroup: Codable, Identifiable {
    public var id: UUID
    public var name: String
    public var active: Bool
}
