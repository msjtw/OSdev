"""M-021 current-working-directory and AT_FDCWD regressions."""

import re

import pytest


pytestmark = pytest.mark.qemu


def test_chdir_updates_relative_path_resolution(qemu):
    assert qemu.run("cd /bin").status == 0

    cwd = qemu.run("pwd")
    assert cwd.status == 0
    assert re.search(r"(?m)^/bin\r?$", cwd.output), cwd.output

    assert qemu.run("./true").status == 0
    parent = qemu.run("cat ../lorem.txt")
    assert parent.status == 0
    assert "Lorem ipsum dolor sit amet" in parent.output


def test_chdir_rejects_regular_files_without_changing_cwd(qemu):
    assert qemu.run("cd /bin").status == 0
    assert qemu.run("cd /lorem.txt").status != 0

    cwd = qemu.run("pwd")
    assert cwd.status == 0
    assert re.search(r"(?m)^/bin\r?$", cwd.output), cwd.output
