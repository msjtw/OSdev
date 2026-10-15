"""Host-side serial-console harness for QEMU kernel tests."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import os
import queue
import re
import shutil
import signal
import subprocess
import threading
import time
import uuid


class QemuHarnessError(RuntimeError):
    """Base error for host-side QEMU test failures."""


class QemuTimeout(QemuHarnessError):
    """QEMU did not produce an expected serial marker before the deadline."""


@dataclass(frozen=True)
class CommandResult:
    command: str
    output: str
    status: int


class QemuSession:
    """A running QEMU instance controlled through its UART serial stream.

    The guest protocol deliberately uses shell output markers instead of the
    interactive shell prompt. Prompt text is configuration-dependent, whereas
    a marker emitted by ``printf`` is unambiguous and machine-readable.
    """

    def __init__(
        self,
        *,
        kernel: Path,
        image: Path,
        qemu: str = "qemu-system-riscv32",
        boot_timeout: float = 20.0,
        command_timeout: float = 5.0,
        boot_marker: str = "into the schedulervere",
    ) -> None:
        self.kernel = kernel
        self.image = image
        self.qemu = qemu
        self.boot_timeout = boot_timeout
        self.command_timeout = command_timeout
        self.boot_marker = boot_marker
        self.process: subprocess.Popen[bytes] | None = None
        self._output: queue.Queue[bytes | None] = queue.Queue()
        self._reader: threading.Thread | None = None
        self._transcript = bytearray()

    @property
    def transcript(self) -> str:
        return self._transcript.decode("utf-8", errors="replace")

    def start(self) -> None:
        if self.process is not None:
            raise QemuHarnessError("QEMU session already started")
        if shutil.which(self.qemu) is None:
            raise QemuHarnessError(f"QEMU binary not found: {self.qemu}")
        if not self.kernel.is_file():
            raise QemuHarnessError(f"kernel binary not found: {self.kernel}")
        if not self.image.is_file():
            raise QemuHarnessError(f"root filesystem image not found: {self.image}")

        command = [
            self.qemu,
            "-machine",
            "virt",
            "-global",
            "virtio-mmio.force-legacy=false",
            "-display",
            "none",
            "-monitor",
            "none",
            "-serial",
            "stdio",
            "-kernel",
            os.fspath(self.kernel),
            "-drive",
            f"file={self.image},if=none,format=raw,id=rootfs",
            "-device",
            "virtio-blk-device,drive=rootfs",
        ]
        self.process = subprocess.Popen(
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        assert self.process.stdout is not None
        self._reader = threading.Thread(
            target=self._read_output,
            args=(self.process.stdout,),
            name="qemu-serial-reader",
            daemon=True,
        )
        self._reader.start()

    def boot(self) -> None:
        """Wait for the current kernel's scheduler log, then verify shell I/O."""
        self._wait_for(self.boot_marker.encode(), self.boot_timeout)
        token = self._token("READY")
        self._send(f"printf '{token}\\n'\n")
        self._wait_for_match(
            re.compile(rb"(?m)^" + re.escape(token.encode()) + rb"\r?\n"),
            self.boot_timeout,
        )

    def run(self, command: str, *, timeout: float | None = None) -> CommandResult:
        """Run one shell command and return its serial output and exit status."""
        if "\n" in command:
            raise ValueError("run() accepts one shell command line")
        token = self._token("DONE")
        marker = f"{token}:"
        self._send(
            f"{command}; __qemu_status=$?; "
            f"printf '{marker}%s\\n' \"$__qemu_status\"\n"
        )
        match, output = self._wait_for_match(
            re.compile(re.escape(marker).encode() + rb"(-?\d+)(?:\r?\n)"),
            timeout or self.command_timeout,
        )
        return CommandResult(command=command, output=output, status=int(match.group(1)))

    def save_artifacts(self, directory: Path, test_name: str) -> Path:
        """Persist the serial transcript and rootfs image for failure analysis."""
        directory.mkdir(parents=True, exist_ok=True)
        stem = f"{test_name}-{int(time.time())}"
        log_path = directory / f"{stem}.log"
        image_path = directory / f"{stem}.ext4"
        log_path.write_text(self.transcript)
        shutil.copy2(self.image, image_path)
        return log_path

    def close(self) -> None:
        process = self.process
        if process is None:
            return
        self.process = None
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=2)
        if self._reader is not None:
            self._reader.join(timeout=1)

    def _read_output(self, stream) -> None:
        try:
            while chunk := stream.read(1):
                self._output.put(chunk)
        finally:
            self._output.put(None)

    def _send(self, text: str) -> None:
        if self.process is None or self.process.stdin is None:
            raise QemuHarnessError("QEMU session is not running")
        if self.process.poll() is not None:
            raise QemuHarnessError(f"QEMU exited with status {self.process.returncode}\n{self.transcript}")
        self.process.stdin.write(text.encode())
        self.process.stdin.flush()

    def _wait_for(self, marker: bytes, timeout: float) -> str:
        _, output = self._wait_for_match(re.compile(re.escape(marker)), timeout)
        return output

    def _wait_for_match(self, pattern: re.Pattern[bytes], timeout: float):
        deadline = time.monotonic() + timeout
        observed = bytearray()
        while True:
            match = pattern.search(observed)
            if match is not None:
                return match, observed.decode("utf-8", errors="replace")
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise QemuTimeout(
                    f"timed out after {timeout:.1f}s waiting for {pattern.pattern!r}\n"
                    f"serial output:\n{self.transcript}"
                )
            try:
                chunk = self._output.get(timeout=remaining)
            except queue.Empty as error:
                raise QemuTimeout(
                    f"timed out after {timeout:.1f}s waiting for {pattern.pattern!r}\n"
                    f"serial output:\n{self.transcript}"
                ) from error
            if chunk is None:
                status = self.process.returncode if self.process is not None else "unknown"
                raise QemuHarnessError(f"QEMU exited with status {status}\n{self.transcript}")
            observed.extend(chunk)
            self._transcript.extend(chunk)

    @staticmethod
    def _token(kind: str) -> str:
        return f"__QEMU_{kind}_{uuid.uuid4().hex}__"
