// swift-tools-version:5.7
import PackageDescription

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
            path: "Sources/Fingerprint"
        ),
        .executableTarget(
            name: "FingerprintBenchmarkRunner",
            dependencies: ["Fingerprint"],
            path: "Benchmarks/FingerprintBenchmarkRunner"
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
