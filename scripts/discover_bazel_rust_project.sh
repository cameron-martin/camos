#!/usr/bin/bash

set -euo pipefail

# Workaround for https://github.com/bazelbuild/rules_rust/issues/3447
bazel --output_user_root=~/.cache/bazel_ra fetch --repo=@@rules_rust++rust+rust_analyzer_1.86.0_tools 2>/dev/null

bazel \
    run \
    @rules_rust//tools/rust_analyzer:discover_bazel_rust_project -- \
    --bazel_startup_option=--output_user_root=~/.cache/bazel_ra \
    --bazel_arg=--watchfs \
    ${1:+"$1"} 2>/dev/null
