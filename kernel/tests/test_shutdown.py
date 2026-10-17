"""Minimal BusyBox poweroff integration on QEMU virt."""

import pytest


pytestmark = pytest.mark.qemu


def test_busybox_poweroff_exits_qemu(qemu):
    qemu._send("poweroff\n")
    qemu._wait_for(b"poweroff", 1)
    assert qemu.process is not None
    assert qemu.process.wait(timeout=5) == 0
