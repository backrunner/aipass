// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "AipassTray",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "AipassTray", type: .static, targets: ["AipassTray"])
    ],
    targets: [
        .target(name: "AipassTray", path: "Sources/AipassTray"),
        .executableTarget(
            name: "TrayPreview",
            dependencies: ["AipassTray"],
            path: "Sources/TrayPreview"
        ),
    ]
)
