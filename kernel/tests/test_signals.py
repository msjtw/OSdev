"""M-033 RV32 signal-frame delivery and rt_sigreturn regression."""

import os
import shutil
import subprocess

import pytest

from conftest import ROOT
from qemu_harness import QemuSession


@pytest.mark.qemu
def test_signal_handler_and_rt_sigreturn(tmp_path, kernel_binary, rootfs_image, request):
    for tool in ("clang", "debugfs"):
        if shutil.which(tool) is None:
            pytest.fail(f"M-033 tests require host tool: {tool}")
    sysroot = subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()
    host = next(
        line.removeprefix("host: ")
        for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines()
        if line.startswith("host: ")
    )
    obj = tmp_path / "m033-signal.o"
    binary = tmp_path / "m033-signal"
    subprocess.run([
        "clang", "--target=riscv32-unknown-none-elf", "-march=rv32ima", "-mabi=ilp32",
        "-c", str(ROOT / "tests/user/signal.S"), "-o", str(obj),
    ], check=True)
    linker = os.path.join(sysroot, "lib", "rustlib", host, "bin", "rust-lld")
    subprocess.run([
        linker, "-flavor", "gnu", "-m", "elf32lriscv", "-Ttext=0x10000", "--entry=_start",
        str(obj), "-o", str(binary),
    ], check=True)

    image = tmp_path / "rootfs.ext4"
    shutil.copy2(rootfs_image, image)
    subprocess.run([
        "debugfs", "-w", "-R", f"write {binary} /m033-signal", str(image),
    ], check=True, capture_output=True, text=True)
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
        session._send("/m033-signal\n")
        session._wait_for(b"M033_READY", 5)
        session._send("\x03")
        session._wait_for(b"M033_HANDLER_OK", 5)
        session._wait_for(b"M033_SIGNAL_TEST_OK", 5)
    except Exception:
        session.close()
        session.save_artifacts(ROOT / "build/test-artifacts", "m033-signal")
        raise
    finally:
        session.close()
