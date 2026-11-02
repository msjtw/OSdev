# UART server

Build with `sh build.sh`; the script prints the ELF path. The fixed heap defaults
to 64 KiB (`UART_HEAP_SIZE` overrides it at link time). Host tests: `cargo test`.

Uses serverlib startup, arguments, panic handling, allocator, and IPC. Requires
exactly one argument: the mapped UART virtual base in decimal or 0x/0X hex.
Only kernel-sender raw read/write requests are accepted. No terminal policy,
private runtime, allocation syscalls, or hard-coded device-address fallback.

For standalone tests, pass `UART_DRIVER_BIN=/path/to/ELF` to kernel/mkrootfs.sh
to install `/sbin/uart`. The kernel separately builds/embeds UART at PID 3 with
its MMIO page mapped; normal kernel TTY transport has not yet switched to IPC.
Ordinary rootfs execution does not grant access to the UART device.
