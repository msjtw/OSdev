### Hello, World of {os_name}!

## Building BusyBox (RISC-V 32-bit, musl)

BusyBox is pinned to upstream **1.38.0**. Needs a
`riscv32-unknown-linux-musl-` cross-toolchain.

From the repository root:

```sh
git submodule update --init --recursive
cp configs/busybox.config busybox-1.38.0/.config
make -C busybox-1.38.0 oldconfig
make -C busybox-1.38.0 -j"$(nproc)"
```

The saved configuration builds a statically linked RV32 executable using musl.
The binary is `busybox-1.38.0/busybox`.

To create the kernel's rootfs image with programs from busybox (requires `mkfs.ext4` from e2fsprogs):

```sh
./kernel/mkrootfs.sh
```


### Changing the configuration

```sh
make -C busybox-1.38.0 menuconfig
cp busybox-1.38.0/.config configs/busybox.config
```
