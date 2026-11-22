"""Build and exercise the real TTY event loop without switching terminal routing."""

import os
from pathlib import Path
import shutil
import subprocess

import pytest

from conftest import ROOT
from qemu_harness import QemuSession
from test_server_layout import elf_layout


@pytest.mark.qemu
@pytest.mark.parametrize("heap_size", [4096, 16384, None],
                         ids=["small-heap", "larger-heap", "embedded"])
def test_tty_server(tmp_path, kernel_binary, rootfs_image, request, heap_size):
    if heap_size is not None:
        driver = ROOT.parent / "tty_driver"
        env = dict(os.environ, TTY_HEAP_SIZE=str(heap_size))
        binary = Path(subprocess.check_output(
            ["sh", str(driver / "build.sh")], cwd=driver, env=env, text=True,
        ).strip())
        _, entry, segments, symbols = elf_layout(binary)
        assert entry == symbols["_start"]
        start, end = symbols["__server_heap_start"], symbols["__server_heap_end"]
        assert end - start == heap_size
        assert start % 4096 == 0
        assert any(
            p[0] == 1 and p[6] & 2 and p[2] <= start and end <= p[2] + p[5]
            and start >= p[2] + p[4]
            for p in segments
        )  # Fixed writable NOLOAD heap, not an allocator-backed VM request.

    sysroot = subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()
    host = next(line.removeprefix("host: ") for line in subprocess.check_output(
        ["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
    obj, probe = tmp_path / "tty.o", tmp_path / "tty-probe"
    subprocess.run([
        "clang", "--target=riscv32-unknown-none-elf", "-march=rv32ima", "-mabi=ilp32",
        "-ffreestanding", "-fno-builtin", "-fno-stack-protector", "-msmall-data-limit=0",
        *(["-DTTY_BOOTSTRAP"] if heap_size is None else []),
        "-O1", "-c", str(ROOT / "tests/user/tty_server.c"), "-o", str(obj),
    ], check=True)
    subprocess.run([
        str(Path(sysroot) / "lib/rustlib" / host / "bin/rust-lld"),
        "-flavor", "gnu", "-m", "elf32lriscv", "-Ttext=0x10000", "--entry=_start",
        str(obj), "-o", str(probe),
    ], check=True)
    image = tmp_path / "rootfs.ext4"
    shutil.copy2(rootfs_image, image)
    sources = [(probe, "tty-probe")]
    if heap_size is not None:
        sources.insert(0, (binary, "tty-driver"))
    for source, name in sources:
        installed = subprocess.run([
            "debugfs", "-w", "-R", f"write {source} /{name}", str(image),
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
        result = session.run('echo "$$"')
        assert result.status == 0 and "1" in result.output.replace("\r", "").splitlines(), result.output
        if heap_size is None:
            assert session.run("test ! -e /tty-driver && test ! -e /sbin/uart").status == 0
        for args in ([] if heap_size is None else ["", "''", "0", "0x0", "oops", "-1", "4294967296",
                     "0x100000000", "2 extra", "2 1", "2 0 1", "2 1 0",
                     "2 bad 1", "2 1 4294967296", "2 1 1 extra"]):
            result = session.run(f"/tty-driver {args}")
            assert result.status == 2, result.output
        result = session.run("/tty-probe", timeout=20)
        assert result.status == 0, result.output
        assert "TTY_SERVER_OK" in result.output
        if heap_size is None:
            assert "TTY_UART_WRITE_OK" in result.output
        assert session.run("echo console-still-works").status == 0
    except Exception:
        session.close()
        session.save_artifacts(ROOT / "build/test-artifacts", "tty-server")
        raise
    finally:
        session.close()
