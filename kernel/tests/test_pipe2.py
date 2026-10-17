"""M-030 pipe2 regressions, including BusyBox pipeline integration."""

import os
import re
import shutil
import subprocess

import pytest

from conftest import ROOT
from qemu_harness import QemuSession


@pytest.mark.qemu
def test_pipe2_eof_epipe_and_busybox_pipeline(tmp_path, kernel_binary, rootfs_image, request):
    for tool in ("clang", "debugfs"):
        if shutil.which(tool) is None:
            pytest.fail(f"M-030 tests require host tool: {tool}")
    sysroot = subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()
    host = next(
        line.removeprefix("host: ")
        for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines()
        if line.startswith("host: ")
    )
    binary = tmp_path / "m030-pipe2"
    obj = tmp_path / "m030-pipe2.o"
    subprocess.run([
        "clang", "--target=riscv32-unknown-none-elf", "-march=rv32ima", "-mabi=ilp32",
        "-c", str(ROOT / "tests/user/pipe2.S"), "-o", str(obj),
    ], check=True)
    linker = os.path.join(sysroot, "lib", "rustlib", host, "bin", "rust-lld")
    subprocess.run([
        linker, "-flavor", "gnu", "-m", "elf32lriscv", "-Ttext=0x10000", "--entry=_start",
        str(obj), "-o", str(binary),
    ], check=True)
    image = tmp_path / "rootfs.ext4"
    shutil.copy2(rootfs_image, image)
    subprocess.run([
        "debugfs", "-w", "-R", f"write {binary} /m030-pipe2", str(image),
    ], check=True, capture_output=True, text=True)
    session = QemuSession(
        kernel=kernel_binary,
        image=image,
        qemu=os.environ.get("QEMU_SYSTEM_RISCV32", "qemu-system-riscv32"),
        boot_timeout=request.config.getoption("--qemu-boot-timeout"),
        command_timeout=request.config.getoption("--qemu-command-timeout"),
        boot_marker=os.environ.get("QEMU_BOOT_MARKER", "into the schedulervere"),
    )
    try:
        session.start()
        session.boot()
        result = session.run("/m030-pipe2", timeout=30)
        assert result.status == 0, result.output
        assert re.search(r"(?m)^M030_PIPE2_TEST_OK\r?$", result.output), result.output
        pipeline = session.run("printf M030_PIPELINE_OK | cat")
        assert pipeline.status == 0, pipeline.output
        assert "M030_PIPELINE_OK" in pipeline.output
    except Exception:
        session.close()
        session.save_artifacts(ROOT / "build/test-artifacts", "m030-pipe2")
        raise
    finally:
        session.close()
