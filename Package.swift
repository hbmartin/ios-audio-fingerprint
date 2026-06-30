// swift-tools-version:5.7
import PackageDescription

let package = Package(
    name: "Fingerprint",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "Fingerprint", targets: ["Fingerprint"]),
    ],
    targets: [
        .target(
            name: "Fingerprint",
            dependencies: [],
            path: "Sources/Fingerprint"
        ),
        .binaryTarget(
            name: "FingerprintFFI",
            path: "Fingerprint.xcframework"
        ),
        .testTarget(
            name: "FingerprintTests",
            dependencies: ["Fingerprint"]
        ),
    ]
)
