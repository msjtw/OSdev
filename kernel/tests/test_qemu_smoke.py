from __future__ import annotations

import re

import pytest

from qemu_harness import QemuTimeout


pytestmark = pytest.mark.qemu


def test_boot_and_serial_command(qemu):
    assert "BusyBox v" in qemu.transcript
    assert "built-in shell (ash)" in qemu.transcript
    result = qemu.run("echo harness-ok")
    assert result.status == 0
    assert re.search(r"(?m)^harness-ok\r?$", result.output)


def test_list_root_directory(qemu):
    result = qemu.run("ls -1 --color=never /")
    assert result.status == 0
    entries = result.output.replace("\r", "").splitlines()
    assert "bin" in entries
    assert "lorem.txt" in entries


def test_read_rootfs_file(qemu):
    result = qemu.run("cat /lorem.txt")
    assert result.status == 0
    assert "Lorem ipsum dolor sit amet, consectetur adipiscing elit." in result.output
    assert "Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua." in result.output


def test_execute_busybox_child(qemu):
    result = qemu.run("/bin/true")
    assert result.status == 0


def test_failed_child_exit_status(qemu):
    result = qemu.run("/bin/false")
    assert result.status == 1
    # A failing child must not prevent the shell from executing another command.
    result = qemu.run("/bin/true")
    assert result.status == 0


def test_command_timeout_is_reported(qemu):
    with pytest.raises(QemuTimeout):
        qemu.run("while :; do :; done", timeout=0.2)
