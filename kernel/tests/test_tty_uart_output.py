"""IPC-021: userspace UART output, echo, write errors, and interruption."""

import os
import re
import shutil
import subprocess
import time

import pytest

from conftest import ROOT
from qemu_harness import QemuSession


pytestmark = pytest.mark.qemu


def test_tty_write_completion_errors_and_recovery(tmp_path, kernel_binary, rootfs_image, request):
    obj = tmp_path / "tty-output.o"
    binary = tmp_path / "tty-output"
    subprocess.run([
        "clang", "--target=riscv32-unknown-none-elf", "-march=rv32ima", "-mabi=ilp32",
        "-ffreestanding", "-fno-builtin", "-fno-stack-protector", "-msmall-data-limit=0",
        "-O1", "-c", str(ROOT / "tests/user/tty_output.c"), "-o", str(obj),
    ], check=True)
    subprocess.run([
        "rust-lld", "-flavor", "gnu", "-m", "elf32lriscv", "-Ttext=0x10000", "--entry=_start",
        str(obj), "-o", str(binary),
    ], check=True)
    image = tmp_path / "rootfs.ext4"
    shutil.copy2(rootfs_image, image)
    installed = subprocess.run(["debugfs", "-w", "-R", f"write {binary} /tty-output", str(image)],
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
        result = session.run("/tty-output", timeout=30)
        assert result.status == 0, result.output
        assert re.search(r"(?m)^IPC021_WRITEV_OK\r?$", result.output)
        assert "w" * 8192 in result.output
        assert re.search(r"(?m)^IPC021_WRITE_OK\r?$", result.output)
        assert session.run("echo output-still-works").status == 0
    except Exception:
        session.close()
        session.save_artifacts(ROOT / "build/test-artifacts", "tty-output")
        raise
    finally:
        session.close()


def test_async_echo_wakes_reader_and_keeps_exact_bytes(qemu):
    for index in range(4):
        qemu._send("stty icanon echo; printf 'IPC021_ECHO_WAIT\\n'; "
                   "read line; printf 'IPC021_ECHO_DONE:%s\\n' \"$line\"\n")
        qemu._wait_for_match(re.compile(rb"(?m)^IPC021_ECHO_WAIT\r?$"), 5)
        time.sleep(0.2)
        text = f"echo-data-{index}"
        qemu._send(text + "\n")
        qemu._wait_for_match(re.compile(rb"(?m)^" + re.escape(text.encode()) + rb"\r?$"), 5)
        qemu._wait_for_match(re.compile(rb"(?m)^IPC021_ECHO_DONE:" + re.escape(text.encode()) + rb"\r?$"), 5)


def test_ctrl_c_stops_foreground_writer_and_later_output_works(qemu):
    qemu._send("yes IPC021_BUSY\n")
    qemu._wait_for_match(re.compile(rb"(?m)^IPC021_BUSY\r?$"), 5)
    qemu._send("\x03")
    result = qemu.run("printf 'IPC021_AFTER_SIGNAL\\n'")
    assert result.status == 0, result.output
    assert re.search(r"(?m)^IPC021_AFTER_SIGNAL\r?$", result.output)
