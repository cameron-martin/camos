load(":defs.bzl", "dummy_cc_config")

# Dummy UEFI toolchain
toolchain(
    name = "cc-toolchain-x86_64-none",
    exec_compatible_with = [
        "@platforms//cpu:x86_64",
        "@platforms//os:linux",
    ],
    target_compatible_with = [
        "@platforms//cpu:x86_64",
        "@platforms//os:none",
    ],
    target_settings = None,
    toolchain = ":cc-dummy-x86_64-none",
    toolchain_type = "@bazel_tools//tools/cpp:toolchain_type",
)

dummy_cc_config(
    name = "cc_toolchain_config",
)

cc_toolchain(
    name = "cc-dummy-x86_64-none",
    all_files = ":empty",
    compiler_files = ":empty",
    dwp_files = ":empty",
    linker_files = ":empty",
    objcopy_files = ":empty",
    strip_files = ":empty",
    # supports_param_files = 0,
    toolchain_config = ":cc_toolchain_config",
    toolchain_identifier = "dummy_wasm_cc",
)

filegroup(
    name = "empty",
    srcs = [],
    visibility = ["//:__subpackages__"],
)