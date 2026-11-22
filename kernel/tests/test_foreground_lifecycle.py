"""M-032 foreground child, pipeline, and Ctrl-C lifecycle regressions."""

import re

import pytest


pytestmark = pytest.mark.qemu


def test_foreground_pipeline_reaping_reuses_process_slots(qemu):
    for _ in range(40):
        # Fresh-image diagnosis observed completion at 5.082 seconds after the
        # old five-second deadline, without any recovery input. Keep a bounded
        # stress-test deadline allowing userspace terminal scheduling overhead.
        result = qemu.run("printf M032_PIPELINE | cat | cat", timeout=15)
        assert result.status == 0, result.output
        assert "M032_PIPELINE" in result.output


def test_ctrl_c_interrupts_foreground_child_not_shell(qemu):
    qemu._send("sh -c 'printf \"M032_CHILD_READY\\n\"; sleep 10'\n")
    # Command echo does not imply that ash has started the foreground child or
    # restored signal-generating terminal mode, especially with async echo.
    qemu._wait_for_match(re.compile(rb"(?m)^M032_CHILD_READY\r?$"), 5)
    qemu._send("\x03")
    result = qemu.run("printf M032_SHELL_STILL_ALIVE")
    assert result.status == 0, result.output
    assert "M032_SHELL_STILL_ALIVE" in result.output, result.output
