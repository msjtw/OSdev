#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
KERNEL=${1:?"Cargo did not provide the kernel executable"}
shift
IMAGE=${ROOTFS_IMAGE:-"$ROOT/build/rootfs.ext4"}

if [ ! -f "$IMAGE" ]; then
    "$ROOT/mkrootfs.sh" "$IMAGE"
fi

exec qemu-system-riscv32 \
    -machine virt \
    -global virtio-mmio.force-legacy=false \
    -nographic \
    -kernel "$KERNEL" \
    -drive "file=$IMAGE,if=none,format=raw,id=rootfs" \
    -device virtio-blk-device,drive=rootfs
