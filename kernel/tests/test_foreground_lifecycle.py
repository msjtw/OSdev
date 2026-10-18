"""M-032 foreground child, pipeline, and Ctrl-C lifecycle regressions."""

import pytest


pytestmark = pytest.mark.qemu


def test_foreground_pipeline_reaping_reuses_process_slots(qemu):
    for _ in range(40):
        result = qemu.run("printf M032_PIPELINE | cat | cat")
        assert result.status == 0, result.output
        assert "M032_PIPELINE" in result.output


def test_ctrl_c_interrupts_foreground_child_not_shell(qemu):
    qemu._send("sleep 10\n")
    qemu._wait_for(b"sleep 10", 2)
    qemu._send("\x03")
    qemu._wait_for(b"/ #", 5)

    result = qemu.run("printf M032_SHELL_STILL_ALIVE")
    assert result.status == 0, result.output
    assert "M032_SHELL_STILL_ALIVE" in result.output, result.output
