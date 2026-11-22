"""IPC-030: complete UART demo on a newly built BusyBox-only ext4 image."""

import os
import re
import subprocess
import time

import pytest

from conftest import ROOT
from qemu_harness import QemuSession


@pytest.mark.qemu
def test_uart_demo_on_fresh_busybox_only_rootfs(tmp_path, kernel_binary, request):
    image = tmp_path / "rootfs.ext4"
    env = dict(os.environ)
    env.pop("UART_DRIVER_BIN", None)
    subprocess.run(["sh", str(ROOT / "mkrootfs.sh"), str(image)],
                   cwd=ROOT, env=env, check=True, capture_output=True, text=True)
    listing = subprocess.run(["debugfs", "-R", "stat /sbin/uart", str(image)],
                             capture_output=True, text=True, check=True)
    assert "File not found" in listing.stderr
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
        assert session.run("test ! -e /sbin/uart").status == 0
        time.sleep(0.3)  # Exercise idle/empty input polling before more commands.
        result = session.run("printf 'alpha\\nbeta\\n' | cat | wc -l")
        assert result.status == 0, result.output
        assert re.search(r"(?m)^2\r?$", result.output)
        result = session.run(
            "sh -c 'exec 3>/dev/tty; printf \"IPC030_FD_OK\\n\" >&3; "
            "exec 3>&-; printf \"IPC030_STDERR_OK\\n\" >&2'"
        )
        assert result.status == 0, result.output
        assert re.search(r"(?m)^IPC030_FD_OK\r?$", result.output)
        assert re.search(r"(?m)^IPC030_STDERR_OK\r?$", result.output)

        # Canonical input is the boot default. BusyBox stty uses unsupported
        # TCSETSW/TCSADRAIN; direct TCSETS coverage lives in tty_semantics.
        session._send("printf 'IPC030_INPUT_WAIT\\n'; "
                      "read line; printf 'IPC030_INPUT:%s\\n' \"$line\"\n")
        session._wait_for_match(re.compile(rb"(?m)^IPC030_INPUT_WAIT\r?$"), 5)
        payload = "fresh-0123456789" * 40
        session._send(payload + "\r")
        session._wait_for_match(
            re.compile(rb"(?m)^IPC030_INPUT:" + re.escape(payload.encode()) + rb"\r?$"), 20,
        )
        session._send("yes IPC030_STREAM\n")
        session._wait_for_match(re.compile(rb"(?m)^IPC030_STREAM\r?$"), 5)
        session._send("\x03")
        result = session.run("printf 'IPC030_DEMO_OK\\n'")
        assert result.status == 0, result.output
        assert re.search(r"(?m)^IPC030_DEMO_OK\r?$", result.output)
    except Exception:
        session.close()
        session.save_artifacts(ROOT / "build/test-artifacts", "uart-demo")
        raise
    finally:
        session.close()
