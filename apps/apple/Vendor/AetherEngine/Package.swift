// swift-tools-version: 6.0
// AetherEngine 7.36.0 (https://github.com/superuser404notfound/AetherEngine/tree/7.36.0), vendored for
// Marquee (plans/apple-second-engine.md): the engine library alone, its dependencies at the exact versions
// 7.36.0 resolved, and the patch listed in VENDOR.md.

import PackageDescription

let package = Package(
    name: "AetherEngine",
    platforms: [
        .iOS(.v18),
        .tvOS(.v18),
        .macOS(.v15),
    ],
    products: [
        .library(name: "AetherEngine", targets: ["AetherEngine"]),
    ],
    dependencies: [
        .package(url: "https://github.com/superuser404notfound/FFmpegBuild", exact: "3.7.1"),
        .package(url: "https://github.com/superuser404notfound/LibDovi", exact: "2.1.0"),
        .package(url: "https://github.com/superuser404notfound/LibTrueHD", exact: "2.3.0"),
        .package(url: "https://github.com/superuser404notfound/LibDTSX", exact: "1.0.1"),
    ],
    targets: [
        .target(
            name: "AetherEngine",
            dependencies: [
                .product(name: "AetherFFmpegBuild", package: "FFmpegBuild"),
                .product(name: "Dovi", package: "LibDovi"),
                .product(name: "TrueHD", package: "LibTrueHD"),
                .product(name: "DTSX", package: "LibDTSX"),
            ],
            resources: [.copy("Resources/silent-atmos.mp4")],
            linkerSettings: [
                .linkedFramework("AVFoundation"),
                .linkedFramework("AVKit"),
                .linkedFramework("CoreMedia"),
                .linkedFramework("CoreVideo"),
                .linkedFramework("VideoToolbox"),
                .linkedFramework("AudioToolbox"),
            ]
        ),
    ]
)
