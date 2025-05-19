load("@crates//:defs.bzl", "aliases")
load("@rules_rust//rust:defs.bzl", "rust_binary")

config_setting(
    name = "debug_mode",
    values = {"compilation_mode": "dbg"},
)

rust_binary(
    name = "camos_uefi",
    srcs = glob(["src/**/*.rs"]),
    aliases = aliases(),
    compile_data = [
        "//crates/camos_kernel",
        ":trampoline",
    ],
    platform = "//platforms:x86_uefi",
    rustc_env = {
        "KERNEL_PATH": "$(execpath //crates/camos_kernel)",
        "TRAMPOLINE_PATH": "$(execpath :trampoline)",
    },
    rustc_flags = ["-Cpanic=abort"] + select({
        ":debug_mode": [
            "-C",
            "link-args=/debug:dwarf",
        ],
        "//conditions:default": [],
    }),
    visibility = ["//visibility:public"],
    deps = [
        "//crates/camos_bootinfo",
        "@crates//:elf",
        "@crates//:thiserror",
        "@crates//:uart_16550",
        "@crates//:uefi",
        "@crates//:x86_64",
    ],
)

genrule(
    name = "trampoline",
    srcs = ["trampoline.asm"],
    outs = ["trampoline.bin"],
    cmd = "nasm -f bin $(location trampoline.asm) -o $@",
)
