// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "DivvunFST",
    products: [
        .library(
            name: "DivvunFST",
            targets: ["DivvunFST"]
        ),
    ],
    targets: [
        .target(
            name: "DivvunFST",
            dependencies: ["CDivvunFST"]
        ),
        .systemLibrary(
            name: "CDivvunFST",
            path: "Sources/CDivvunFST"
        ),
        .testTarget(
            name: "DivvunFSTTests",
            dependencies: ["DivvunFST"]
        ),
    ]
)
