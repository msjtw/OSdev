"""M-007 user-space regressions; no test hooks are compiled into the kernel."""

import os
import re
import shutil
import struct
import subprocess

import pytest

from conftest import ROOT
from qemu_harness import QemuSession


CASES = [
    ("path-pointer", 1),
    ("argv-pointer", 2),
    ("argument-pointer", 3),
    ("argument-count", 4),
    ("argument-size", 5),
    ("short-elf", 6),
    ("header-table", 6),
]


@pytest.mark.qemu
@pytest.mark.parametrize("name,case", CASES, ids=[name for name, _ in CASES])
def test_failed_exec_returns_to_caller(name, case, tmp_path, kernel_binary, rootfs_image, request):
    for tool in ("clang", "debugfs"):
        if shutil.which(tool) is None:
            pytest.fail(f"M-007 tests require host tool: {tool}")
    sysroot = subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()
    host = next(
        line.removeprefix("host: ")
        for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines()
        if line.startswith("host: ")
    )
    linker = os.path.join(sysroot, "lib", "rustlib", host, "bin", "rust-lld")
    obj = tmp_path / "exec.o"
    binary = tmp_path / "exec-test"
    subprocess.run([
        "clang", "--target=riscv32-unknown-none-elf", "-march=rv32ima",
        "-mabi=ilp32", f"-DCASE={case}", "-c",
        str(ROOT / "tests/user/exec_errors.S"), "-o", str(obj),
    ], check=True)
    subprocess.run([
        linker, "-flavor", "gnu", "-m", "elf32lriscv", "-Ttext=0x10000",
        "--entry=_start", str(obj), "-o", str(binary),
    ], check=True)

    bad = bytearray(binary.read_bytes())
    if name == "short-elf":
        bad = bytearray(b"\x7fELF")
    elif name == "header-table":
        struct.pack_into("<I", bad, 28, 0xfffffff0)  # e_phoff
    malformed = tmp_path / "bad-elf"
    malformed.write_bytes(bad)

    image = tmp_path / "rootfs.ext4"
    shutil.copy2(rootfs_image, image)
    for source, destination in ((binary, "/m007-exec"), (malformed, "/m007-bad-elf")):
        installed = subprocess.run([
            "debugfs", "-w", "-R", f"write {source} {destination}", str(image),
        ], capture_output=True, text=True, check=True)
        assert "Allocated inode:" in installed.stdout, installed.stdout + installed.stderr

    session = QemuSession(
        kernel=kernel_binary,
        image=image,
        qemu=os.environ.get("QEMU_SYSTEM_RISCV32", "qemu-system-riscv32"),
        boot_timeout=request.config.getoption("--qemu-boot-timeout"),
        command_timeout=request.config.getoption("--qemu-command-timeout"),
        boot_marker=os.environ.get("QEMU_BOOT_MARKER", "into the schedulervere"),
    )
    try:
        session.start()
        session.boot()
        result = session.run("/m007-exec", timeout=30)
        assert result.status == 0, result.output
        assert re.search(r"(?m)^M007_EXEC_TEST_OK\r?$", result.output), result.output
        assert session.run("/bin/true").status == 0
        assert "Something went wrong" not in session.transcript
    except Exception:
        session.close()
        session.save_artifacts(ROOT / "build/test-artifacts", f"m007-{name}")
        raise
    finally:
        session.close()
