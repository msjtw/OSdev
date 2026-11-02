"""Exercise serverlib IPC with two ordinary common-runtime executables."""

import os
import shutil
import subprocess

import pytest

from conftest import ROOT
from qemu_harness import QemuSession


@pytest.mark.qemu
def test_serverlib_syscalls(tmp_path, kernel_binary, rootfs_image, request):
    program = ROOT / "tests/user/server_runtime"
    target = "riscv32ima-unknown-none-elf"
    env = dict(os.environ)
    script = ROOT.parent / "serverlib/linker/server.ld"
    env["RUSTFLAGS"] = (
        f"-C link-arg=-T{script} -C link-arg=--defsym=SERVER_HEAP_SIZE=4096 "
        "-C link-arg=--no-relax"
    )
    subprocess.run([
        "cargo", "+nightly", "build", "--target", target,
        "-Z", "build-std=core,alloc", "--target-dir", str(tmp_path / "target"),
    ], cwd=program, env=env, check=True)
    binary = tmp_path / "target" / target / "debug/server-runtime-test"
    image = tmp_path / "rootfs.ext4"
    shutil.copy2(rootfs_image, image)
    installed = subprocess.run([
        "debugfs", "-w", "-R", f"write {binary} /serverlib-test", str(image),
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
        result = session.run(
            '/serverlib-test ipc-server & server=$!; '
            '/serverlib-test ipc-client "$server"; client_status=$?; '
            'wait "$server"; server_status=$?; '
            'test "$client_status" -eq 0 && test "$server_status" -eq 0',
            timeout=30,
        )
        assert result.status == 0, result.output
        assert "SERVERLIB_IPC_OK" in result.output
        assert "SERVER_RUNTIME_SERVER_OK" in result.output
        assert session.run("/bin/true").status == 0
    except Exception:
        session.close()
        session.save_artifacts(ROOT / "build/test-artifacts", "serverlib")
        raise
    finally:
        session.close()
