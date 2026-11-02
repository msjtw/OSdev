#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
IMAGE=${1:-"$ROOT/build/rootfs.ext4"}
BUSYBOX_DIR=${BUSYBOX_DIR:-"$ROOT/../busybox-1.35.0"}
UART_DRIVER_BIN=${UART_DRIVER_BIN:-}

if [ ! -f "$BUSYBOX_DIR/Makefile" ]; then
    echo "BusyBox source tree not found: $BUSYBOX_DIR" >&2
    echo "Set BUSYBOX_DIR=/path/to/busybox-source" >&2
    exit 1
fi

if [ -z "$UART_DRIVER_BIN" ]; then
    UART_DRIVER_BIN=$(sh "$ROOT/../uart_driver/build.sh")
fi
if [ ! -f "$UART_DRIVER_BIN" ]; then
    echo "UART server ELF not found: $UART_DRIVER_BIN" >&2
    exit 1
fi

STAGING=$(mktemp -d)
trap 'rm -rf "$STAGING"' EXIT

# BusyBox owns its installed layout and applet links. Use hard links because
# the kernel filesystem currently does not resolve symbolic links on exec.
make -C "$BUSYBOX_DIR" \
    CONFIG_PREFIX="$STAGING" \
    CONFIG_INSTALL_APPLET_SYMLINKS=n \
    CONFIG_INSTALL_APPLET_HARDLINKS=y \
    install >/dev/null

mkdir -p "$STAGING/sbin"
install -m 755 "$UART_DRIVER_BIN" "$STAGING/sbin/uart"

cat >"$STAGING/lorem.txt" <<'EOF'
Lorem ipsum dolor sit amet, consectetur adipiscing elit.
Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.
EOF

mkdir -p "$(dirname -- "$IMAGE")"
rm -f "$IMAGE"
truncate -s 32M "$IMAGE"
mkfs.ext4 -q -F -b 4096 -d "$STAGING" "$IMAGE"

echo "Created $IMAGE from BusyBox's installed rootfs"
