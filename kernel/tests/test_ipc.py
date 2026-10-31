"""IPC-006 standalone regressions for transport, cancellation, and payload copies."""

import os
import shutil
import subprocess

import pytest

from conftest import ROOT
from qemu_harness import QemuSession


@pytest.mark.qemu
@pytest.mark.parametrize("case", [0, 1, 2], ids=["roundtrip", "missing-server", "empty-receive"])
def test_ipc_transport(case, tmp_path, kernel_binary, rootfs_image, request):
    _run_ipc_program("ipc.S", case, "IPC_TRANSPORT_OK", tmp_path,
                     kernel_binary, rootfs_image, request)


@pytest.mark.qemu
@pytest.mark.parametrize("case", range(7), ids=[
    "delivered-cancel", "queued-cancel", "receive-reuse", "masked-signal",
    "ignored-signal", "default-ignored-sigchld", "fatal-cancel",
])
def test_ipc_signal_cancellation(case, tmp_path, kernel_binary, rootfs_image, request):
    _run_ipc_program("ipc_signals.S", case, "IPC_SIGNALS_OK", tmp_path,
                     kernel_binary, rootfs_image, request)


@pytest.mark.qemu
def test_ipc_payload_copies(tmp_path, kernel_binary, rootfs_image, request):
    _run_ipc_program("ipc_payload.c", 0, "IPC_PAYLOAD_OK", tmp_path,
                     kernel_binary, rootfs_image, request)


@pytest.mark.qemu
@pytest.mark.parametrize("clients", [3, 18], ids=["small-inbox", "growing-inbox"])
def test_ipc_multiple_clients_fifo(clients, tmp_path, kernel_binary, rootfs_image, request):
    _run_ipc_program("ipc_fifo.c", clients, "IPC_FIFO_OK", tmp_path,
                     kernel_binary, rootfs_image, request)


def _run_ipc_program(source, case, marker, tmp_path, kernel_binary, rootfs_image, request):
    for tool in ("clang", "debugfs"):
        if shutil.which(tool) is None:
            pytest.fail(f"IPC tests require host tool: {tool}")
    sysroot = subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()
    host = next(line.removeprefix("host: ") for line in subprocess.check_output(
        ["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
    obj = tmp_path / "ipc.o"
    binary = tmp_path / "ipc"
    cflags = (["-ffreestanding", "-fno-builtin", "-fno-stack-protector",
               "-msmall-data-limit=0", "-O1"] if source.endswith(".c") else [])
    subprocess.run([
        "clang", "--target=riscv32-unknown-none-elf", "-march=rv32ima", "-mabi=ilp32",
        *cflags,
        f"-DCASE={case}", "-c", str(ROOT / "tests/user" / source), "-o", str(obj),
    ], check=True)
    linker = os.path.join(sysroot, "lib", "rustlib", host, "bin", "rust-lld")
    subprocess.run([
        linker, "-flavor", "gnu", "-m", "elf32lriscv", "-Ttext=0x10000", "--entry=_start",
        str(obj), "-o", str(binary),
    ], check=True)
    image = tmp_path / "rootfs.ext4"
    shutil.copy2(rootfs_image, image)
    installed = subprocess.run([
        "debugfs", "-w", "-R", f"write {binary} /ipc-test", str(image),
    ], check=True, capture_output=True, text=True)
    assert "Allocated inode:" in installed.stdout, installed.stdout + installed.stderr
    session = QemuSession(
        kernel=kernel_binary, image=image,
        qemu=os.environ.get("QEMU_SYSTEM_RISCV32", "qemu-system-riscv32"),
        boot_timeout=request.config.getoption("--qemu-boot-timeout"),
        command_timeout=request.config.getoption("--qemu-command-timeout"),
        boot_marker=os.environ.get("QEMU_BOOT_MARKER", "into the schedulervere"),
    )
    try:
        session.start()
        session.boot()
        result = session.run("/ipc-test", timeout=30)
        assert result.status == 0, result.output
        assert marker in result.output
        assert session.run("/bin/true").status == 0
    except Exception:
        session.close()
        session.save_artifacts(ROOT / "build/test-artifacts", f"{source}-{case}")
        raise
    finally:
        session.close()
