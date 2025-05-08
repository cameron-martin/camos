load("@crates//:defs.bzl", "aliases")
load("@rules_rust//rust:defs.bzl", "rust_binary")

platform(
    name = "x86_uefi",
    constraint_values = [
        "@platforms//cpu:x86_64",
        "@platforms//os:uefi",
    ],
)

rust_binary(
    name = "camos_uefi",
    srcs = glob(["src/**/*.rs"]),
    aliases = aliases(),
    compile_data = ["//crates/camos_kernel"],
    # Consider using the platform-data rule here. See https://github.com/bazelbuild/platforms/blob/main/experimental/platform_data/defs.bzl
    # Or here? https://github.com/bazelbuild/rules_platform
    platform = ":x86_uefi",
    rustc_env = {"KERNEL_PATH": "$(execpath //crates/camos_kernel)"},
    rustc_flags = ["-Cpanic=abort"],
    deps = [
        "//crates/camos_bootinfo",
        "@crates//:elf",
        "@crates//:thiserror",
        "@crates//:uefi",
        "@crates//:x86_64",
    ],
)
