from __future__ import annotations

from pathlib import Path
import os
import shutil
import subprocess

import pytest

from qemu_harness import QemuSession


ROOT = Path(__file__).resolve().parents[1]
KERNEL_BINARY = ROOT / "target/riscv32ima-unknown-none-elf/debug/main"
BASE_IMAGE = ROOT / "build/rootfs.ext4"


def pytest_addoption(parser: pytest.Parser) -> None:
    parser.addoption(
        "--qemu-boot-timeout",
        type=float,
        default=float(os.environ.get("QEMU_BOOT_TIMEOUT", "20")),
        help="seconds allowed for QEMU to boot to the BusyBox shell",
    )
    parser.addoption(
        "--qemu-command-timeout",
        type=float,
        default=float(os.environ.get("QEMU_COMMAND_TIMEOUT", "5")),
        help="seconds allowed for one guest shell command",
    )


@pytest.fixture(scope="session")
def kernel_binary() -> Path:
    subprocess.run(["cargo", "build"], cwd=ROOT, check=True)
    if not KERNEL_BINARY.is_file():
        raise RuntimeError(f"Cargo did not produce {KERNEL_BINARY}")
    return KERNEL_BINARY


@pytest.fixture(scope="session")
def rootfs_image() -> Path:
    if not BASE_IMAGE.is_file():
        subprocess.run([ROOT / "mkrootfs.sh", BASE_IMAGE], cwd=ROOT, check=True)
    return BASE_IMAGE


@pytest.fixture
def qemu(
    request: pytest.FixtureRequest,
    tmp_path: Path,
    kernel_binary: Path,
    rootfs_image: Path,
) -> QemuSession:
    image = tmp_path / "rootfs.ext4"
    shutil.copy2(rootfs_image, image)
    session = QemuSession(
        kernel=kernel_binary,
        image=image,
        qemu=os.environ.get("QEMU_SYSTEM_RISCV32", "qemu-system-riscv32"),
        boot_timeout=request.config.getoption("--qemu-boot-timeout"),
        command_timeout=request.config.getoption("--qemu-command-timeout"),
        boot_marker=os.environ.get("QEMU_BOOT_MARKER", "into the schedulervere"),
    )
    session.start()
    try:
        session.boot()
        yield session
    finally:
        report = getattr(request.node, "rep_call", None)
        if report is not None and report.failed:
            session.save_artifacts(ROOT / "build/test-artifacts", request.node.name)
        session.close()


@pytest.hookimpl(hookwrapper=True)
def pytest_runtest_makereport(item: pytest.Item, call: pytest.CallInfo[object]):
    outcome = yield
    report = outcome.get_result()
    setattr(item, f"rep_{report.when}", report)
