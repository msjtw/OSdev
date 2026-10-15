"""Fixed process/descriptor capacity and slot reuse (M-008 subset)."""

import os
import re
import shutil
import subprocess

import pytest

from conftest import ROOT
from qemu_harness import QemuSession


@pytest.mark.qemu
@pytest.mark.parametrize("kind,case,args", [
    ("processes", 1, ""),
    ("fds", 2, ""),
    ("wait-nohang", 3, ""),
    ("stack-argc1", 4, ""),
    ("stack-argc2", 4, " a"),
    ("stack-argc3", 4, " a bb"),
    ("stack-argc4", 4, " a bb ccc"),
])
def test_fixed_resource_limits(kind, case, args, tmp_path, kernel_binary, rootfs_image, request):
    for tool in ("clang", "debugfs"):
        if shutil.which(tool) is None:
            pytest.fail(f"Resource-limit tests require host tool: {tool}")
    sysroot = subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()
    host = next(
        line.removeprefix("host: ")
        for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines()
        if line.startswith("host: ")
    )
    linker = os.path.join(sysroot, "lib", "rustlib", host, "bin", "rust-lld")
    obj = tmp_path / "limits.o"
    binary = tmp_path / "limits-test"
    subprocess.run([
        "clang", "--target=riscv32-unknown-none-elf", "-march=rv32ima",
        "-mabi=ilp32", f"-DCASE={case}", "-c",
        str(ROOT / "tests/user/resource_limits.S"), "-o", str(obj),
    ], check=True)
    subprocess.run([
        linker, "-flavor", "gnu", "-m", "elf32lriscv", "-Ttext=0x10000",
        "--entry=_start", str(obj), "-o", str(binary),
    ], check=True)
    image = tmp_path / "rootfs.ext4"
    shutil.copy2(rootfs_image, image)
    installed = subprocess.run([
        "debugfs", "-w", "-R", f"write {binary} /m008-limits", str(image),
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
        result = session.run("/m008-limits" + args, timeout=30)
        assert result.status == 0, result.output
        assert re.search(r"(?m)^M008_LIMITS_TEST_OK\r?$", result.output), result.output
        assert session.run("/bin/true").status == 0
        assert "Something went wrong" not in session.transcript
    except Exception:
        session.close()
        session.save_artifacts(ROOT / "build/test-artifacts", f"m008-{kind}")
        raise
    finally:
        session.close()
