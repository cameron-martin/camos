load("@crates//:defs.bzl", "aliases")
load("@rules_rust//rust:defs.bzl", "rust_binary")

platform(
    name = "x86_none",
    constraint_values = [
        "@platforms//cpu:x86_64",
        "@platforms//os:none",
    ],
)

rust_binary(
    name = "camos_kernel",
    srcs = glob(["src/**/*.rs"]),
    aliases = aliases(),
    platform = ":x86_none",
    rustc_flags = ["-Cpanic=abort"],
    visibility = ["//visibility:public"],
    deps = ["//crates/camos_bootinfo"],
)
