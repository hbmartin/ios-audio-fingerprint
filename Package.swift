// swift-tools-version:6.2
import PackageDescription

let swiftSettings: [SwiftSetting] = [
    .swiftLanguageMode(.v6),
    .enableUpcomingFeature("ExistentialAny"),
    .enableUpcomingFeature("MemberImportVisibility"),
    .enableUpcomingFeature("InternalImportsByDefault"),
]

let package = Package(
    name: "Fingerprint",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "Fingerprint", targets: ["Fingerprint"]),
        .executable(name: "FingerprintBenchmarkRunner", targets: ["FingerprintBenchmarkRunner"]),
    ],
    targets: [
        .target(
            name: "Fingerprint",
            dependencies: ["FingerprintFFI"],
            path: "Sources/Fingerprint",
            swiftSettings: swiftSettings
        ),
        .executableTarget(
            name: "FingerprintBenchmarkRunner",
            dependencies: ["Fingerprint"],
            path: "Benchmarks/FingerprintBenchmarkRunner",
            swiftSettings: swiftSettings
        ),
        .binaryTarget(
            name: "FingerprintFFI",
            path: "Fingerprint.xcframework"
        ),
        .testTarget(
            name: "FingerprintTests",
            dependencies: ["Fingerprint"],
            swiftSettings: swiftSettings
        ),
    ]
)
