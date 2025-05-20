# --- begin runfiles.bash initialization v3 ---
# Copy-pasted from the Bazel Bash runfiles library v3.
set -uo pipefail; set +e; f=bazel_tools/tools/bash/runfiles/runfiles.bash
# shellcheck disable=SC1090
source "${RUNFILES_DIR:-/dev/null}/$f" 2>/dev/null || \
source "$(grep -sm1 "^$f " "${RUNFILES_MANIFEST_FILE:-/dev/null}" | cut -f2- -d' ')" 2>/dev/null || \
source "$0.runfiles/$f" 2>/dev/null || \
source "$(grep -sm1 "^$f " "$0.runfiles_manifest" | cut -f2- -d' ')" 2>/dev/null || \
source "$(grep -sm1 "^$f " "$0.exe.runfiles_manifest" | cut -f2- -d' ')" 2>/dev/null || \
{ echo>&2 "ERROR: cannot find $f"; exit 1; }; f=; set -e
# --- end runfiles.bash initialization v3 ---

efi_file="$(rlocation $1)"
image_dir="$(mktemp -d)"
image_efi_path=$image_dir/EFI/Boot/BootX64.efi

echo "Booting from $image_dir"

mkdir -p "$(dirname $image_efi_path)"
cp "$efi_file" "$image_efi_path"
chmod 0755 "$image_efi_path"

extra_args=""
if [[ "${2-}" == "debug" ]]; then
    extra_args="-s -S"
    echo "Running in debug mode. To attach gdb, run:"
    echo "  gdb -ex 'target remote localhost:1234' -ex 'dir $BUILD_WORKSPACE_DIRECTORY/bazel-$(basename $BUILD_WORKSPACE_DIRECTORY)' $efi_file"
    echo "  lldb -o 'gdb-remote localhost:1234' "

fi

qemu-system-x86_64 -no-reboot -m 1G -nographic -drive file=fat:rw:$image_dir,format=raw -bios OVMF.fd $extra_args


rm -fr $image_dir
