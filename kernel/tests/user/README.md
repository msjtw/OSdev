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

## Exec failure regressions (M-007)

```sh
uv run pytest tests/test_exec_errors.py -v
```

Uses the same compiler/linker and disposable-image setup as M-006.
`exec_errors.S` attempts each failure 20 times in the same process, checking
the exact errno. It then prints `M007_EXEC_TEST_OK` and successfully execs
`/bin/true`; returning from that final exec is a test failure. Pytest also
checks that the shell can run another command afterward.

Cases cover invalid path/argv/argument pointers (`EFAULT`), more than 64
arguments or more than 16 KiB of argument data (`E2BIG`), and malformed
executables (`ENOEXEC`). ELF cases cover a truncated header and an out-of-file
program-header table. No environment variables or in-kernel test code are used.

Path-length, inconsistent segment-size, reserved-address, and invalid-entry tests
are deliberately excluded:
the project assumes conventional toolchain-generated ELF images rather than
comprehensive validation of malformed executables.

## Fixed resource limits (M-008 subset)

```sh
uv run pytest tests/test_resource_limits.py -v
```

`resource_limits.S` checks a 32-slot system process table (30 children plus
the shell and test parent), `EAGAIN` on exhaustion, reaping, and successful
subsequent cloning. The FD case fills slots 3--63, checks `EMFILE` for open
and dup, rejects dup3 to slot 64, and verifies close/reuse through open, dup,
and dup3, including reuse of the last slot. Both cases leave the shell usable.
These tests do not cover general memory exhaustion or allocation failures.

The same runner also checks `WNOHANG` with a child still sleeping, followed by
a successful blocking wait. Startup-stack checks execute with argc 1--4 and
require a 16-byte-aligned SP with argc and argv still accessible at entry.
