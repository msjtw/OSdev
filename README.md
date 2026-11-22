### Hello, World of DomestOS!

## Building BusyBox (RISC-V 32-bit, musl)

BusyBox is a Git submodule pinned to upstream **1.38.0**. Install a
`riscv32-unknown-linux-musl-` cross-toolchain and put its `bin` directory on
`PATH`. You also need GNU make and a host C compiler.

From the repository root:

```sh
git submodule update --init --recursive
cp configs/busybox.config busybox-1.38.0/.config
make -C busybox-1.38.0 oldconfig
make -C busybox-1.38.0 -j"$(nproc)"
```

The saved configuration builds a statically linked RV32 executable using musl.
The binary is `busybox-1.38.0/busybox`. Build outputs and `.config` are ignored
by the submodule; do not force-add generated files.

To create the kernel's rootfs image (requires `mkfs.ext4` from e2fsprogs):

```sh
./kernel/mkrootfs.sh
```

This installs BusyBox with hard-linked applets, as required by the kernel's
current filesystem implementation.

### Changing the configuration

`menuconfig` requires host ncurses development headers:

```sh
make -C busybox-1.38.0 menuconfig
cp busybox-1.38.0/.config configs/busybox.config
```

Commit configuration changes in `configs/busybox.config`, not generated files
or local changes inside the submodule. Rebuild BusyBox after changing its
configuration. Restoring the saved configuration with `cp` overwrites any
unsaved local configuration changes.
