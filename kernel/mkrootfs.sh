#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
IMAGE=${1:-"$ROOT/build/rootfs.ext4"}
BUSYBOX=${BUSYBOX:-"$ROOT/../../busybox-1.35.0/busybox"}

if [ ! -f "$BUSYBOX" ]; then
    echo "BusyBox binary not found: $BUSYBOX" >&2
    echo "Set BUSYBOX=/path/to/riscv32-busybox" >&2
    exit 1
fi

mkdir -p "$(dirname -- "$IMAGE")"
rm -f "$IMAGE"
truncate -s 32M "$IMAGE"
mkfs.ext4 -q -F -b 4096 "$IMAGE"

# debugfs creates the directory and copies the statically linked RISC-V ELF
# without mounting the image on the host.
debugfs -w -R 'mkdir /bin' "$IMAGE" >/dev/null
debugfs -w -R "write $BUSYBOX /bin/busybox" "$IMAGE" >/dev/null

echo "Created $IMAGE containing /bin/busybox"
