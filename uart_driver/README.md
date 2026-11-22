# UART server

`uart_driver <mmio-address> [tty-pid]` uses serverlib startup and a fixed heap
(64 KiB by default, overridden at build time with `UART_HEAP_SIZE`).
The kernel embeds its ELF, allocates its actual PID and maps UART MMIO only
into this server. Kernel requests and the configured TTY peer are authorized.

The server owns byte transfers, not terminal policy. Reads from TTY are deferred
until input exists; writes use direct volatile stores without transmitter-ready
polling. The kernel timer only marks work; scheduler context coalesces POLL IPC
with one outstanding notification and collects acknowledgements. TTY does not
periodically poll hardware.

TTY owns submitted input/output buffers until completion consumption or confirmed
cancellation. Kernel copy/reply validation rejects revoked access and protects
newer requests from stale IDs. There are no cancellation notifications, automatic
restarts or readiness handshakes. Dead peers may leave operations waiting.

Kernel diagnostics remain direct and can interleave with userspace output.
This does not extract VFS, process management or VM into servers.
