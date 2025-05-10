sh_binary(
    name = "qemu",
    srcs = ["scripts/run_qemu.sh"],
    args = [
        "$(rlocationpath //crates/camos_uefi)",
    ],
    data = [
        "//crates/camos_uefi",
    ],
    deps = ["@bazel_tools//tools/bash/runfiles"],
)

sh_binary(
    name = "qemu_debug",
    srcs = ["scripts/run_qemu.sh"],
    args = [
        "$(rlocationpath //crates/camos_uefi)",
        "debug",
    ],
    data = [
        "//crates/camos_uefi",
    ],
    deps = ["@bazel_tools//tools/bash/runfiles"],
)
