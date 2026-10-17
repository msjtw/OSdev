"""M-025 cross-cutting VFS coverage for synthetic filesystem nodes."""

import pytest


pytestmark = pytest.mark.qemu


def _entries(qemu, path):
    result = qemu.run(f"ls -1 --color=never {path}")
    assert result.status == 0, result.output
    return set(line for line in result.output.replace("\r", "").splitlines() if line.isidentifier())


def test_synthetic_directory_entries_and_metadata(qemu):
    assert _entries(qemu, "/dev") == {"console", "tty", "null", "zero"}
    assert _entries(qemu, "/proc") == {"uptime", "meminfo", "version", "mounts"}

    devices = qemu.run("ls -ld /dev/null /dev/zero")
    assert devices.status == 0, devices.output
    assert devices.output.count("crw-rw-rw-") == 2, devices.output

    proc_file = qemu.run("ls -ld /proc/version")
    assert proc_file.status == 0, proc_file.output
    assert "-r--r--r--" in proc_file.output, proc_file.output


def test_pseudofs_write_policy(qemu):
    assert qemu.run("printf discard >/dev/null").status == 0
    assert qemu.run("printf discard >/dev/zero").status == 0
    for path in ("/proc/uptime", "/proc/meminfo", "/proc/version", "/proc/mounts"):
        result = qemu.run(f"printf denied >{path}")
        assert result.status != 0, result.output
        assert "Read-only file system" in result.output
