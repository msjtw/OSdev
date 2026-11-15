"""IPC-020: real UART input reaches sleepers through scheduler-side IPC work."""

import re
import time

import pytest


pytestmark = pytest.mark.qemu


def test_uart_input_progress_after_idle_empty_reads(qemu):
    # No runnable client is needed to submit polls or consume their replies.
    time.sleep(0.4)
    for index in range(3):
        result = qemu.run(f"printf 'IPC020_IDLE_{index}\\n'")
        assert result.status == 0, result.output
        assert re.search(rf"(?m)^IPC020_IDLE_{index}\r?$", result.output)
        time.sleep(0.2)


@pytest.mark.parametrize("payload,ending", [("awake", "\r"), ("0123456789" * 80, "\n")],
                         ids=["cr-to-newline", "larger-than-poll-batch"])
def test_uart_input_wakes_canonical_reader(qemu, payload, ending):
    qemu._send(
        "stty icanon -echo; printf 'IPC020_WAITING\\n'; "
        "read line; printf 'IPC020_INPUT:%s\\n' \"$line\"\n"
    )
    qemu._wait_for_match(re.compile(rb"(?m)^IPC020_WAITING\r?$"), 5)
    # Let the shell sleep in read, UART sleep in receive, and scheduler idle.
    time.sleep(0.3)
    qemu._send(payload + ending)
    qemu._wait_for_match(
        re.compile(rb"(?m)^IPC020_INPUT:" + re.escape(payload.encode()) + rb"\r?$"), 20,
    )
    result = qemu.run("printf 'IPC020_SHELL_ALIVE\\n'")
    assert result.status == 0, result.output
    assert re.search(r"(?m)^IPC020_SHELL_ALIVE\r?$", result.output)
