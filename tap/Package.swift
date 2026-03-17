// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "termwave-tap",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(
            name: "termwave-tap",
            path: "Sources",
            linkerSettings: [
                .linkedFramework("ScreenCaptureKit"),
                .linkedFramework("CoreMedia"),
                .linkedFramework("AVFoundation"),
            ]
        ),
    ]
)
