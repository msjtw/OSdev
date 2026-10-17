"""M-021 lexical pathname resolution before chdir support is added."""

import pytest


pytestmark = pytest.mark.qemu


@pytest.mark.parametrize("path", [
    "./lorem.txt",
    "//lorem.txt",
    "/bin/../lorem.txt",
    "/../../lorem.txt",
])
def test_path_components_are_normalized(qemu, path):
    result = qemu.run(f"cat {path}")
    assert result.status == 0, result.output
    assert "Lorem ipsum dolor sit amet" in result.output


def test_normalized_executable_path(qemu):
    assert qemu.run("/bin/./true").status == 0
