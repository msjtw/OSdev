"""Common-runtime UART installation and embedded boot, without VM test hooks."""

import os
from pathlib import Path
import shutil
import subprocess

import pytest

from conftest import ROOT
from qemu_harness import QemuSession


@pytest.mark.qemu
@pytest.mark.parametrize("address", ["0x10000000", "268435456", "0x20000000", None],
                         ids=["hex", "decimal", "alternate", "embedded-boot"])
def test_uart_server_build_install_and_receive_loop(address, tmp_path, kernel_binary, request):
    for tool in ("clang", "debugfs", "mkfs.ext4", "make"):
        if shutil.which(tool) is None:
            pytest.fail(f"UART server tests require host tool: {tool}")
    driver = ROOT.parent / "uart_driver"
    binary = Path(subprocess.check_output(["sh", str(driver / "build.sh")],
                                         cwd=driver, text=True).strip())
    image = tmp_path / "rootfs.ext4"
    env = dict(os.environ)
    if address is None:
        env.pop("UART_DRIVER_BIN", None)
    else:
        env["UART_DRIVER_BIN"] = str(binary)
    subprocess.run(["sh", str(ROOT / "mkrootfs.sh"), str(image)],
                   cwd=ROOT, env=env, check=True, capture_output=True, text=True)
    installed = tmp_path / "installed-uart"
    subprocess.run(["debugfs", "-R", f"dump /sbin/uart {installed}", str(image)],
                   check=True, capture_output=True, text=True)
    if address is None:
        assert not installed.exists()  # UART must boot without a rootfs executable.
    else:
        assert installed.read_bytes() == binary.read_bytes()

    sysroot = subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()
    host = next(line.removeprefix("host: ") for line in subprocess.check_output(
        ["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
    obj = tmp_path / "uart-probe.o"
    probe = tmp_path / "uart-probe"
    subprocess.run([
        "clang", "--target=riscv32-unknown-none-elf", "-march=rv32ima", "-mabi=ilp32",
        "-ffreestanding", "-fno-builtin", "-fno-stack-protector", "-msmall-data-limit=0",
        *(["-DUART_BOOTSTRAP"] if address is None else [f'-DUART_ADDRESS_ARG="{address}"']),
        "-O1", "-c", str(ROOT / "tests/user/uart_server.c"), "-o", str(obj),
    ], check=True)
    linker = os.path.join(sysroot, "lib", "rustlib", host, "bin", "rust-lld")
    subprocess.run([
        linker, "-flavor", "gnu", "-m", "elf32lriscv", "-Ttext=0x10000", "--entry=_start",
        str(obj), "-o", str(probe),
    ], check=True)
    result = subprocess.run(["debugfs", "-w", "-R", f"write {probe} /uart-probe", str(image)],
                            check=True, capture_output=True, text=True)
    assert "Allocated inode:" in result.stdout, result.stdout + result.stderr

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
        for command in (() if address is None else (
            "/sbin/uart", "/sbin/uart ''", "/sbin/uart invalid",
            "/sbin/uart 0x100000000", "/sbin/uart 4294967296",
            "/sbin/uart -1", "/sbin/uart 0x10000000 extra",
        )):
            assert session.run(command).status == 2
        result = session.run("/uart-probe", timeout=30)
        assert result.status == 0, result.output
        assert ("UART_BOOTSTRAP_OK" if address is None else "UART_SERVER_OK") in result.output
        assert session.run("echo console-still-works").status == 0
    except Exception:
        session.close()
        session.save_artifacts(ROOT / "build/test-artifacts", "uart-server")
        raise
    finally:
        session.close()


def test_rootfs_build_does_not_require_uart(tmp_path):
    image = tmp_path / "busybox-only.ext4"
    env = dict(os.environ)
    env.pop("UART_DRIVER_BIN", None)
    subprocess.run(["sh", str(ROOT / "mkrootfs.sh"), str(image)],
                   cwd=ROOT, env=env, check=True, capture_output=True, text=True)
    result = subprocess.run(["debugfs", "-R", "stat /sbin/uart", str(image)],
                            check=True, capture_output=True, text=True)
    assert "File not found" in result.stderr
    result = subprocess.run(["debugfs", "-R", "stat /bin/busybox", str(image)],
                            check=True, capture_output=True, text=True)
    assert "Inode:" in result.stdout
