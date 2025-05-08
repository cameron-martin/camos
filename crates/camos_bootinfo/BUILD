load("@crates//:defs.bzl", "aliases")
load("@rules_rust//rust:defs.bzl", "rust_library")

rust_library(
    name = "camos_bootinfo",
    srcs = glob(["src/**/*.rs"]),
    aliases = aliases(),
    # rustc_flags = ["-Cpanic=abort"],
    visibility = ["//visibility:public"],
    deps = ["@crates//:uefi"],
)
