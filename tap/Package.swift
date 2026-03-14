// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "sonitus-tap",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(
            name: "sonitus-tap",
            path: "Sources",
            linkerSettings: [
                .linkedFramework("ScreenCaptureKit"),
                .linkedFramework("CoreMedia"),
                .linkedFramework("AVFoundation"),
            ]
        ),
    ]
)
