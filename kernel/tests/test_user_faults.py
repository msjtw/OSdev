"""M-006 acceptance tests: intentionally red until user-fault isolation exists."""

import os
import re
import shutil
import subprocess

import pytest

from conftest import ROOT
from qemu_harness import QemuSession


@pytest.mark.qemu
@pytest.mark.parametrize("kind", ["load", "store", "illegal"])
def test_user_fault_isolation(kind, tmp_path, kernel_binary, rootfs_image, request):
    for tool in ("clang", "debugfs"):
        if shutil.which(tool) is None:
            pytest.fail(f"M-006 tests require host tool: {tool}")
    sysroot = subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()
    host = next(
        line.removeprefix("host: ")
        for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines()
        if line.startswith("host: ")
    )
    linker = os.path.join(sysroot, "lib", "rustlib", host, "bin", "rust-lld")
    obj = tmp_path / "fault.o"
    binary = tmp_path / "fault-test"
    number = {"load": 1, "store": 2, "illegal": 3}[kind]
    subprocess.run([
        "clang", "--target=riscv32-unknown-none-elf", "-march=rv32ima",
        "-mabi=ilp32", f"-DFAULT_KIND={number}", "-c",
        str(ROOT / "tests/user/fault.S"), "-o", str(obj),
    ], check=True)
    subprocess.run([
        linker, "-flavor", "gnu", "-m", "elf32lriscv", "-Ttext=0x10000",
        "--entry=_start", str(obj), "-o", str(binary),
    ], check=True)

    image = tmp_path / "rootfs.ext4"
    shutil.copy2(rootfs_image, image)
    installed = subprocess.run([
        "debugfs", "-w", "-R", f"write {binary} /m006-fault", str(image),
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
        result = session.run("/m006-fault", timeout=30)
        assert result.status == 0, result.output
        assert re.search(r"(?m)^M006_FAULT_TEST_OK\r?$", result.output), result.output
        assert session.run("/bin/true").status == 0
        assert "Something went wrong" not in session.transcript
    except Exception:
        session.close()
        session.save_artifacts(ROOT / "build/test-artifacts", f"m006-{kind}")
        raise
    finally:
        session.close()
