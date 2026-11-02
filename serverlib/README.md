# serverlib

Common no_std server support. Use `serverlib::entry!(server_main)` with
`fn server_main(args: serverlib::Args) -> !`. Entry captures the loader's original
stack and initializes gp before Rust code. Args provides `len`, `is_empty`, `get`
(CStr), and allocation-free decimal/0x/0X `parse_usize` (EINVAL for missing/invalid
values, EOVERFLOW for overflow). Signs and whitespace are not accepted.

Link with [linker/server.ld](linker/server.ld), `--no-relax`, and an explicit
nonzero, page-multiple `--defsym=SERVER_HEAP_SIZE=...`. The loader supplies the
16-byte-aligned initial stack containing argc/argv and zeros BSS/NOLOAD memory.

The default panic handler exits 1 without allocation or I/O. Override it with
`serverlib::entry!(server_main, panic = my_panic)` and
`fn my_panic(info: &core::panic::PanicInfo<'_>) -> !`. Do not also define a private
entry, global allocator, or panic handler. Servers choose usage policy
(conventionally exit 2).

Entry installs a buddy allocator and initializes it once from
`__server_heap_start`/`__server_heap_end` before calling server logic. Use
`extern crate alloc` for Vec/Box/String. Freed blocks are reusable; fallible
reservation reports exhaustion. The heap never grows or calls brk/mmap.
Infallible OOM exits 1 without allocation or I/O, independent of panic overrides.
RISC-V userspace builds require nightly for the allocation-error handler; build
core and alloc when using `-Z build-std`.

UART migration remains RT-005. ABI-only dependencies exclude allocator support
and install neither entry nor panic/allocation-error handlers.
