import Foundation

/// Status snapshot pushed from the Rust side as JSON.
/// Field names match the camelCase serde DTO in `tray.rs`.
public struct TrayStatus: Codable {
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
    var proxyRunning: Bool
    var canOpenProxy: Bool
    var canStartProxy: Bool
    var canStopProxy: Bool
    var tooltip: String

    static let checking = TrayStatus(
        agentText: "Agent: checking...",
        agentState: "checking",
        canStartAgent: false,
        canLock: false,
        proxyText: "Status: checking...",
        proxyState: "checking",
        proxyStateText: "Checking…",
        proxyDetail: nil,
        proxyRunning: false,
        canOpenProxy: true,
        canStartProxy: false,
        canStopProxy: false,
        tooltip: "AIPass Agent"
    )
}
