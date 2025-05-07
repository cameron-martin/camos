load("@crates//:defs.bzl", "aliases", "all_crate_deps")
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
    aliases = aliases(),
    deps = all_crate_deps(
        normal = True,
    ),
    proc_macro_deps = all_crate_deps(
        proc_macro = True,
    ),
    srcs = glob(["src/**/*.rs"]),
    rustc_flags = ["-Cpanic=abort"],
    platform = ":x86_none",
    visibility = ["//visibility:public"],
)