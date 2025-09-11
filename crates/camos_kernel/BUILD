load("@crates//:defs.bzl", "aliases")
load("@rules_rust//rust:defs.bzl", "rust_binary", "rust_library", "rust_test")

rust_binary(
    name = "camos_kernel",
    srcs = ["src/main.rs"],
    aliases = aliases(),
    platform = "//platforms:x86_none",
    rustc_flags = [
        "-Cpanic=abort",
        "-Crelocation-model=static",
        # This address range is defined by the SystemV ABI:
        # https://github.com/hjl-tools/x86-psABI/wiki/x86-64-psABI-1.0.pdf
        "-Ccode-model=kernel",
        "-Clink-args=--image-base=0xFFFFFFFF80000000",
    ],
    visibility = ["//visibility:public"],
    deps = [
        ":camos_kernel_lib",
        "//crates/camos_bootinfo",
        "@crates//:uart_16550",
        "@crates//:uefi",
        "@crates//:x86_64",
    ],
)

rust_library(
    name = "camos_kernel_lib",
    srcs = glob(["src/**/*.rs"]),
    aliases = aliases(),
    deps = [
        "@crates//:uart_16550",
        "@crates//:uefi",
        "@crates//:x86_64",
    ],
)

rust_test(
    name = "unit_tests",
    crate = ":camos_kernel_lib",
)

genrule(
    name = "asm",
    srcs = [":camos_kernel"],
    outs = ["camos_kernel.asm"],
    cmd = "objdump -M intel -d $(location :camos_kernel) > $@",
)
