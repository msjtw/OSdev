"""Exercise common startup, arguments, gp initialization, and panic dispatch."""

import os
import shutil
import subprocess

import pytest

from conftest import ROOT
from qemu_harness import QemuSession


@pytest.mark.qemu
@pytest.mark.parametrize("heap_size,profile", [(4096, "debug"), (16384, "release")])
def test_server_runtime(tmp_path, kernel_binary, rootfs_image, request, heap_size, profile):
    target = "riscv32ima-unknown-none-elf"
    env = dict(os.environ)
    script = ROOT.parent / "serverlib/linker/server.ld"
    env["RUSTFLAGS"] = (
        f"-C link-arg=-T{script} -C link-arg=--defsym=SERVER_HEAP_SIZE={heap_size} "
        "-C link-arg=--no-relax"
    )
    image = tmp_path / "rootfs.ext4"
    shutil.copy2(rootfs_image, image)
    for name, features in [("runtime-test", []), ("runtime-custom", ["--features", "custom-panic"])]:
        subprocess.run([
            "cargo", "+nightly", "build", "--target", target, "-Z", "build-std=core,alloc",
            "--target-dir", str(tmp_path / "target"), *features,
            *(["--release"] if profile == "release" else []),
        ], cwd=ROOT / "tests/user/server_runtime", env=env, check=True)
        binary = tmp_path / "target" / target / profile / "server-runtime-test"
        installed = subprocess.run([
            "debugfs", "-w", "-R", f"write {binary} /{name}", str(image),
        ], capture_output=True, text=True, check=True)
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
        for value in ["268435456", "0x10000000", "0X10000000"]:
            result = session.run(f"/runtime-test {value}")
            assert result.status == 0, result.output
            assert "SERVER_RUNTIME_OK" in result.output
        for value in ["", "''", "0x", "oops", "4294967296", "0x100000000", "-1", "1 2"]:
            result = session.run(f"/runtime-test {value}")
            assert result.status == 2, result.output
        assert session.run("/runtime-test panic").status == 1
        assert session.run("/runtime-custom panic").status == 7
        assert session.run("/runtime-custom 268435456").status == 0
        result = session.run("/runtime-test heap")
        assert result.status == 0, result.output
        assert "SERVER_HEAP_OK" in result.output
        assert session.run("/runtime-test oom").status == 1
        assert session.run("/runtime-custom oom").status == 1
        assert session.run("/bin/true").status == 0
    except Exception:
        session.close()
        session.save_artifacts(ROOT / "build/test-artifacts", "server-runtime")
        raise
    finally:
        session.close()
