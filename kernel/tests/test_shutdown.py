"""M-041 sync and controlled BusyBox shutdown integration on QEMU virt."""

import pytest


pytestmark = pytest.mark.qemu


def test_busybox_sync_returns(qemu):
    result = qemu.run("sync")
    assert result.status == 0, result.output


@pytest.mark.parametrize("command", ["poweroff", "halt -f"])
def test_busybox_poweroff_commands_exit_qemu(qemu, command):
    qemu._send(f"{command}\n")
    qemu._wait_for(command.encode(), 1)
    assert qemu.process is not None
    assert qemu.process.wait(timeout=5) == 0


def test_busybox_reboot_restarts_qemu(qemu):
    qemu._send("reboot -f\n")
    qemu._wait_for(b"into the schedulervere", 5)
    assert qemu.process is not None
    assert qemu.process.poll() is None
