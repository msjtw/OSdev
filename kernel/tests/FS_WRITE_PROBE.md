# M-004 synchronous ext4 write probe

Run from the kernel directory:

```sh
uv run pytest tests/test_fs_write_probe.py -v
```

Requires QEMU, the Rust toolchain, and e2fsprogs (`mkfs.ext4`, `dumpe2fs`,
`debugfs`, `e2fsck`). The test creates a disposable 32-MiB filesystem, without
BusyBox, and builds a separate kernel under `target/fs-write-probe/`. It never
modifies `build/rootfs.ext4`. Failed images and UART logs are retained under
`build/test-artifacts/`.

The controlled candidate image command is:

```sh
mkfs.ext4 -q -F -b 4096 -I 256 \
  -O none,extent,filetype,sparse_super,large_file,64bit \
  -E lazy_itable_init=0 -d STAGING IMAGE
```

This is a candidate profile, not yet a validated writable-root profile. The
journal and metadata checksums are disabled. `64bit` describes ext4's on-disk
group-descriptor format; it does not require an RV64 CPU.

## Kernel behavior

Only `--features fs-write-probe` enables adapter writes and the boot probe.
Normal kernels remain read-only. The probe requires a pre-existing
`/m004-probe.txt` of exactly 12288 bytes, overwrites across its last block
boundary, flushes, then creates `/m004-created.txt` and attempts an 8229-byte
write. Success is announced only after the final device flush. Never use a
probe-enabled kernel against a valuable filesystem.

The adapter uses synchronous writes and read-modify-write for partial device
blocks. The VirtIO write-data descriptor is device-readable, while its status
descriptor is device-writable. Missing required VirtIO features now fail
initialization. No syscall write support or production cache has been added.

## Findings: M-004 is blocked on the pinned ext4_rs dependency

Pinned revision: `066bc65689707ba87719ab8ad0657f06f51c4af9`.

1. Without `64bit`, the library divides by zero using an on-disk descriptor
   size of zero instead of using the legacy descriptor size.
2. `write_at` uses unsigned subtraction in its allocation calculation
   (`ext4_impls/file.rs:335`). Overwriting a short range with more file blocks
   remaining than the write needs panics in debug builds. The current probe
   deliberately targets the last two blocks to continue the investigation.
3. File extension fails with `EFBIG` on RV32: the 16-GiB
   `EXT4_MAX_FILE_SIZE` constant is cast to 32-bit `usize` in `write_at`
   (`ext4_impls/file.rs:527`), making the limit zero. The new file is allocated
   but cannot be successfully populated through that API.

The test currently fails on finding 3, intentionally. Do not mark M-004 done
or treat the candidate profile as validated. Next decide whether to maintain a
patched dependency or port to RV64. The unsigned-subtraction bug needs fixing
regardless of architecture. Do not modify Cargo's cached checkout in place.

After resolving the library issues, complete the read-only `e2fsck -fn` and
host content checks in the test, then add a normal-kernel reboot/readback of
the same image with BusyBox installed. Neither filesystem integrity nor that
reboot/readback has yet been proven.
