from __future__ import annotations

from pathlib import Path
import os
import shutil
import subprocess
import hashlib
import json

import pytest

from qemu_harness import QemuSession


ROOT = Path(__file__).resolve().parents[1]
BASE_IMAGE = ROOT / "build/rootfs.ext4"


def pytest_addoption(parser: pytest.Parser) -> None:
    parser.addoption("--rootfs-image", type=Path, default=None,
                     help="base rootfs image to copy for each guest")
    parser.addoption(
        "--kernel-profile", choices=("debug", "release"), default="debug",
        help="kernel build profile (release is faster for full regression runs)",
    )
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
def kernel_binary(request) -> Path:
    profile = request.config.getoption("--kernel-profile")
    subprocess.run(["cargo", "build", *(["--release"] if profile == "release" else [])],
                   cwd=ROOT, check=True)
    binary = ROOT / "target/riscv32ima-unknown-none-elf" / profile / "main"
    if not binary.is_file():
        raise RuntimeError(f"Cargo did not produce {binary}")
    return binary


@pytest.fixture(scope="session")
def server_target_dir(request):
    """Reuse Cargo artifacts, separating linker/features/profile and worker variants."""
    toolchain = subprocess.check_output(["rustc", "+nightly", "-vV"], text=True)
    worker = getattr(request.config, "workerinput", {}).get("workerid", "serial")

    def directory(target, profile, rustflags, features=()):
        configuration = json.dumps(
            [toolchain, target, profile, rustflags, list(features)], sort_keys=True,
        )
        key = hashlib.sha256(configuration.encode()).hexdigest()[:20]
        return ROOT / "build/test-targets" / worker / key

    return directory


@pytest.fixture(scope="session")
def rootfs_image(request) -> Path:
    image = request.config.getoption("--rootfs-image") or BASE_IMAGE
    image = image.resolve()
    if not image.is_file():
        subprocess.run([ROOT / "mkrootfs.sh", image], cwd=ROOT, check=True)
    return image


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
