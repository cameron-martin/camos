load("@crates//:defs.bzl", "aliases")
load("@rules_rust//rust:defs.bzl", "rust_binary")

rust_binary(
    name = "camos_kernel",
    srcs = glob(["src/**/*.rs"]),
    aliases = aliases(),
    platform = "//platforms:x86_none",
    rustc_flags = [
        "-Cpanic=abort",
        "-Clink-args=--image-base=0xFFFFFFFF80000000",
    ],
    visibility = ["//visibility:public"],
    deps = [
        "//crates/camos_bootinfo",
        "@crates//:uart_16550",
    ],
)
