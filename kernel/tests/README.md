Note, the regression tests are vibe-coded.
So they don't prove anything; not that normal tests prove anything.

For faster full runs, use `python -m pytest --kernel-profile=release`.
The default remains `debug`; this option selects the kernel, independently of
any debug/release client variants exercised by individual tests.
Use `--durations=20` to report the slowest cases.

Rust runtime clients reuse Cargo artifacts in `build/test-targets`, separated by
toolchain, target, profile, linker flags (including heap size), features and pytest
worker. Cargo still checks source changes on every build. Test images and QEMU
processes remain isolated; this does not promise safe parallel suite execution.
Independent pytest invocations sharing a checkout should not run concurrently.
