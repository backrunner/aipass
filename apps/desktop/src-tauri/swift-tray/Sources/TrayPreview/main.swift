import AppKit
import AipassTray
import SwiftUI

// Renders the tray panel to a PNG without launching the desktop app.
// Usage: swift run TrayPreview <output.png> [light|dark] [--status status.json]

let args = CommandLine.arguments
guard args.count >= 2 else {
    FileHandle.standardError.write("usage: TrayPreview <output.png> [light|dark] [--status file]\n".data(using: .utf8)!)
    exit(2)
}
let outputPath = args[1]
let dark = args.contains("dark")

let sampleJSON = """
{
  "agentText": "Agent: running (unlocked)",
  "agentState": "unlocked",
  "canStartAgent": false,
  "canLock": true,
  "proxyText": "Status: Running | 127.0.0.1:8787 | 3 routes",
  "proxyState": "running",
  "proxyStateText": "Running",
  "proxyDetail": "127.0.0.1:8787 · 3 routes",
  "proxyRunning": true,
  "canOpenProxy": true,
  "canStartProxy": false,
  "canStopProxy": true,
  "tooltip": "AIPass Agent is running; vault is unlocked"
}
"""

let statusData: Data
if let flag = args.firstIndex(of: "--status"), args.count > flag + 1 {
    statusData = try Data(contentsOf: URL(fileURLWithPath: args[flag + 1]))
} else {
    statusData = Data(sampleJSON.utf8)
}
let status = try JSONDecoder().decode(TrayStatus.self, from: statusData)

_ = NSApplication.shared
let model = TrayViewModel()
model.apply(status: status)

let hosting = NSHostingView(rootView: TrayPanelView(model: model) { action in
    print("action:", action)
})
hosting.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)

let size = hosting.fittingSize
hosting.frame = NSRect(origin: .zero, size: size)
guard let rep = hosting.bitmapImageRepForCachingDisplay(in: hosting.bounds) else {
    FileHandle.standardError.write("failed to create bitmap\n".data(using: .utf8)!)
    exit(1)
}
hosting.cacheDisplay(in: hosting.bounds, to: rep)
guard let png = rep.representation(using: .png, properties: [:]) else {
    FileHandle.standardError.write("failed to encode png\n".data(using: .utf8)!)
    exit(1)
}
try png.write(to: URL(fileURLWithPath: outputPath))
print("wrote \(outputPath) (\(Int(size.width))x\(Int(size.height)))")
