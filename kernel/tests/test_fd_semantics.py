"""M-022 descriptor semantics, exercised by a standalone RV32 program."""

import os
import re
import shutil
import subprocess

import pytest

from conftest import ROOT
from qemu_harness import QemuSession


@pytest.mark.qemu
def test_fd_offsets_dup3_and_cloexec(tmp_path, kernel_binary, rootfs_image, request):
    for tool in ("clang", "debugfs"):
        if shutil.which(tool) is None:
            pytest.fail(f"M-022 tests require host tool: {tool}")
    sysroot = subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()
    host = next(
        line.removeprefix("host: ")
        for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines()
        if line.startswith("host: ")
    )
    linker = os.path.join(sysroot, "lib", "rustlib", host, "bin", "rust-lld")
    binaries = []
    for role, name in ((1, "m022-fd"), (2, "m022-after")):
        obj = tmp_path / f"{name}.o"
        binary = tmp_path / name
        subprocess.run([
            "clang", "--target=riscv32-unknown-none-elf", "-march=rv32ima",
            "-mabi=ilp32", f"-DROLE={role}", "-c",
            str(ROOT / "tests/user/fd_semantics.S"), "-o", str(obj),
        ], check=True)
        subprocess.run([
            linker, "-flavor", "gnu", "-m", "elf32lriscv", "-Ttext=0x10000",
            "--entry=_start", str(obj), "-o", str(binary),
        ], check=True)
        binaries.append((binary, f"/{name}"))
    image = tmp_path / "rootfs.ext4"
    shutil.copy2(rootfs_image, image)
    for binary, destination in binaries:
        installed = subprocess.run([
            "debugfs", "-w", "-R", f"write {binary} {destination}", str(image),
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
        result = session.run("/m022-fd", timeout=30)
        assert result.status == 0, result.output
        assert re.search(r"(?m)^M022_FD_TEST_OK\r?$", result.output), result.output
        assert session.run("/bin/true").status == 0
    except Exception:
        session.close()
        session.save_artifacts(ROOT / "build/test-artifacts", "m022-fd")
        raise
    finally:
        session.close()
