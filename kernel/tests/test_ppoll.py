"""M-031 blocking readiness and ppoll regressions."""

import os
import re
import shutil
import subprocess

import pytest

from conftest import ROOT
from qemu_harness import QemuSession


@pytest.mark.qemu
def test_ppoll_pipe_and_tty_wakeup(tmp_path, kernel_binary, rootfs_image, request):
    for tool in ("clang", "debugfs"):
        if shutil.which(tool) is None:
            pytest.fail(f"M-031 tests require host tool: {tool}")
    sysroot = subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()
    host = next(
        line.removeprefix("host: ")
        for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines()
        if line.startswith("host: ")
    )
    obj = tmp_path / "m031-ppoll.o"
    binary = tmp_path / "m031-ppoll"
    subprocess.run([
        "clang", "--target=riscv32-unknown-none-elf", "-march=rv32ima", "-mabi=ilp32",
        "-c", str(ROOT / "tests/user/ppoll.S"), "-o", str(obj),
    ], check=True)
    linker = os.path.join(sysroot, "lib", "rustlib", host, "bin", "rust-lld")
    subprocess.run([
        linker, "-flavor", "gnu", "-m", "elf32lriscv", "-Ttext=0x10000", "--entry=_start",
        str(obj), "-o", str(binary),
    ], check=True)
    image = tmp_path / "rootfs.ext4"
    shutil.copy2(rootfs_image, image)
    subprocess.run([
        "debugfs", "-w", "-R", f"write {binary} /m031-ppoll", str(image),
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
        result = session.run("/m031-ppoll", timeout=30)
        assert result.status == 0, result.output
        assert re.search(r"(?m)^M031_PPOLL_TEST_OK\r?$", result.output), result.output
        session._send("read line; printf 'M031_TTY_WAKE:%s\\n' \"$line\"\n")
        session._wait_for(b"read line", 2)
        session._send("awake\n")
        session._wait_for_match(re.compile(rb"(?m)^M031_TTY_WAKE:awake\r?$"), 5)
    except Exception:
        session.close()
        session.save_artifacts(ROOT / "build/test-artifacts", "m031-ppoll")
        raise
    finally:
        session.close()
