"""M-023 built-in devfs regressions."""

import re

import pytest


pytestmark = pytest.mark.qemu


def test_devfs_is_listed_and_contains_builtin_devices(qemu):
    root = qemu.run("ls -1 --color=never /")
    assert root.status == 0
    assert "dev" in root.output.replace("\r", "").splitlines()

    devices = qemu.run("ls -1 --color=never /dev")
    assert devices.status == 0
    entries = devices.output.replace("\r", "").splitlines()
    assert {"console", "tty", "null", "zero"}.issubset(entries)
    assert qemu.run("ls /dev/not-real").status != 0


def test_null_zero_console_and_tty(qemu):
    assert qemu.run("printf ignored >/dev/null").status == 0
    assert qemu.run("dd if=/dev/zero of=/dev/null bs=32 count=1 2>/dev/null").status == 0

    console = qemu.run("printf M023_CONSOLE_OK >/dev/console")
    assert console.status == 0
    assert re.search(r"M023_CONSOLE_OK", console.output), console.output

    tty = qemu.run("printf M023_TTY_OK >/dev/tty")
    assert tty.status == 0
    assert re.search(r"M023_TTY_OK", tty.output), tty.output
