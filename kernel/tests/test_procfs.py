"""M-024 built-in read-only procfs regressions."""

import pytest


pytestmark = pytest.mark.qemu


def test_procfs_lists_builtin_files(qemu):
    root = qemu.run("ls -1 --color=never /")
    assert root.status == 0
    assert "proc" in root.output.replace("\r", "").splitlines()

    listing = qemu.run("ls -1 --color=never /proc")
    assert listing.status == 0
    assert {"uptime", "meminfo", "version", "mounts"}.issubset(
        listing.output.replace("\r", "").splitlines()
    )
    assert qemu.run("cat /proc/not-real").status != 0


def test_procfs_content_reaches_eof_and_is_readonly(qemu):
    for path, expected in (
        ("/proc/uptime", "."),
        ("/proc/meminfo", "MemTotal:"),
        ("/proc/version", "rv32 kernel"),
        ("/proc/mounts", "rootfs / ext4 ro"),
    ):
        result = qemu.run(f"cat {path}")
        assert result.status == 0, result.output
        assert expected in result.output, result.output
        assert qemu.run(f"printf changed >{path}").status != 0
