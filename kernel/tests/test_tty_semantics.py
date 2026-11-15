"""IPC-022: termios, canonical/raw readiness, echo, and mode-change wakeups."""

import os
import re
import shutil
import subprocess

import pytest

from conftest import ROOT
from qemu_harness import QemuSession


@pytest.mark.qemu
@pytest.mark.parametrize("case", [1, 2, 3], ids=["modes", "mode-wakes-poll", "mode-wakes-read"])
def test_uart_tty_semantics(case, tmp_path, kernel_binary, rootfs_image, request):
    obj = tmp_path / "tty-semantics.o"
    binary = tmp_path / "tty-semantics"
    subprocess.run([
        "clang", "--target=riscv32-unknown-none-elf", "-march=rv32ima", "-mabi=ilp32",
        "-ffreestanding", "-fno-builtin", "-fno-stack-protector", "-msmall-data-limit=0",
        "-O1", f"-DCASE={case}", "-c", str(ROOT / "tests/user/tty_semantics.c"), "-o", str(obj),
    ], check=True)
    subprocess.run([
        "rust-lld", "-flavor", "gnu", "-m", "elf32lriscv", "-Ttext=0x10000", "--entry=_start",
        str(obj), "-o", str(binary),
    ], check=True)
    image = tmp_path / "rootfs.ext4"
    shutil.copy2(rootfs_image, image)
    installed = subprocess.run(["debugfs", "-w", "-R", f"write {binary} /tty-semantics", str(image)],
                               capture_output=True, text=True, check=True)
    assert "Allocated inode:" in installed.stdout
    session = QemuSession(
        kernel=kernel_binary, image=image,
        qemu=os.environ.get("QEMU_SYSTEM_RISCV32", "qemu-system-riscv32"),
        boot_timeout=request.config.getoption("--qemu-boot-timeout"),
        command_timeout=request.config.getoption("--qemu-command-timeout"),
        boot_marker=os.environ.get("QEMU_BOOT_MARKER", "into the schedulervere"),
    )
    try:
        session.start()
        session.boot()
        session._send("/tty-semantics\n")
        if case == 1:
            session._wait_for_match(re.compile(rb"(?m)^IPC022_CANON_WAIT\r?$"), 5)
            session._send("abc")
            _, output = session._wait_for_match(re.compile(rb"(?m)^IPC022_PARTIAL_OK\r?$"), 3)
            assert "abc" not in output  # ECHO is disabled.
            session._send("\r")
            session._wait_for_match(re.compile(rb"(?m)^IPC022_RAW_WAIT\r?$"), 5)
            session._send("\x03\r")
            session._wait_for_match(re.compile(rb"(?m)^IPC022_ECHO_WAIT\r?$"), 5)
            session._send("loud\n")
        else:
            session._wait_for_match(re.compile(rb"(?m)^IPC022_MODE_WAIT\r?$"), 5)
            session._send("mode")
        _, output = session._wait_for_match(re.compile(rb"(?m)^IPC022_TTY_OK\r?$"), 3)
        if case == 1:
            assert re.search(r"(?m)^loud\r?$", output)
        assert session.run("printf 'IPC022_SHELL_ALIVE\\n'").status == 0
    except Exception:
        session.close()
        session.save_artifacts(ROOT / "build/test-artifacts", f"tty-semantics-{case}")
        raise
    finally:
        session.close()
