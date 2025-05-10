load("@bazel_tools//tools/cpp:cc_toolchain_config_lib.bzl", "action_config", "tool_path", "tool")
load("@rules_cc//cc:action_names.bzl", "ACTION_NAMES")

def _config_impl(ctx):
    tool_paths = [
        tool_path(
            name = "gcc",
            path = "/usr/bin/clang",
        ),
        tool_path(
            name = "ld",
            path = "/usr/bin/ld.lld",
        ),
        tool_path(
            name = "ar",
            path = "/usr/bin/ar",
        ),
        tool_path(
            name = "cpp",
            path = "/bin/false",
        ),
        tool_path(
            name = "gcov",
            path = "/bin/false",
        ),
        tool_path(
            name = "nm",
            path = "/bin/false",
        ),
        tool_path(
            name = "objdump",
            path = "/bin/false",
        ),
        tool_path(
            name = "strip",
            path = "/bin/false",
        ),
    ]

    action_configs = [
        action_config (
            action_name = ACTION_NAMES.cpp_link_executable,
            tools = [
                tool(
                    path = "/usr/bin/ld.lld",
                ),
            ],
        ),
    ]

    return cc_common.create_cc_toolchain_config_info(
        ctx = ctx,
        toolchain_identifier = "dummy-uefi-cc-toolchain",
        host_system_name = "unknown",
        target_system_name = "unknown",
        target_cpu = "unknown",
        target_libc = "unknown",
        compiler = "unknown",
        abi_version = "unknown",
        abi_libc_version = "unknown",
        action_configs = action_configs,
        tool_paths = tool_paths,
    )

dummy_cc_config = rule(
    implementation = _config_impl,
    attrs = {},
    provides = [CcToolchainConfigInfo],
)