# User-space fault tests (M-006)

Run from the kernel directory:

```sh
uv run pytest tests/test_user_faults.py -v
```

Requires `clang` with RISC-V support, Rust's bundled `rust-lld`, `debugfs`,
and the usual QEMU test prerequisites. The assembly uses Linux RV32 syscalls
directly, without a libc dependency. No test code is compiled into the kernel.

Pytest builds three versions of `fault.S`: invalid load, invalid store, and
illegal instruction. Each runs on its own copy of the BusyBox root image;
the original image is never changed.

The parent creates and reaps 40 faulting children sequentially. It requires
`waitid` to report `SIGCHLD`, `CLD_KILLED`, the correct child PID, and
`SIGSEGV` (11) or `SIGILL` (4). A second wait must return `ECHILD`. Repeating
beyond the process-table capacity checks that slots can be reused. The test
then prints a success marker and exits normally; pytest checks that the shell
can still execute `/bin/true`.

These tests intentionally fail until M-006 is implemented. The initial red run
reproduced kernel panics for trap causes 13 (load page fault), 15 (store page
fault), and 2 (illegal instruction). Logs and failed images are saved under
`build/test-artifacts/`. They are not xfailed: failures must become real passes
when fault isolation and signal-aware wait reporting are implemented.
