load("@crates//:defs.bzl", "aliases", "all_crate_deps")
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
    aliases = aliases(),
    deps = all_crate_deps(
        normal = True,
    ) + ["//crates/camos_bootinfo"],
    proc_macro_deps = all_crate_deps(
        proc_macro = True,
    ),
    srcs = glob(["src/**/*.rs"]),
    rustc_flags = ["-Cpanic=abort"],
    compile_data = ["//crates/camos_kernel"],
    rustc_env = { "KERNEL_PATH": "$(execpath //crates/camos_kernel)" },
    # Consider using the platform-data rule here. See https://github.com/bazelbuild/platforms/blob/main/experimental/platform_data/defs.bzl
    # Or here? https://github.com/bazelbuild/rules_platform
    platform = ":x86_uefi",
)