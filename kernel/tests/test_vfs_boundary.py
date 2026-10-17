"""M-020 acceptance checks through the syscall-facing VFS boundary."""

import pytest


pytestmark = pytest.mark.qemu


def test_vfs_directory_and_readonly_file_io(qemu):
    directory = qemu.run("ls -1 --color=never /")
    assert directory.status == 0
    assert "lorem.txt" in directory.output

    write = qemu.run("printf changed >/lorem.txt")
    assert write.status != 0

    read = qemu.run("cat /lorem.txt")
    assert read.status == 0
    assert "Lorem ipsum dolor sit amet" in read.output
