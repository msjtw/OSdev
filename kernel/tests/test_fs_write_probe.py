"""M-004: synchronous ext4 mutations on a disposable controlled image."""

import os
import shutil
import subprocess

import pytest

from conftest import ROOT
from qemu_harness import QemuSession


@pytest.mark.skip
@pytest.mark.qemu
def test_ext4_write_probe(tmp_path, request):
    for tool in ("mkfs.ext4", "debugfs", "dumpe2fs", "e2fsck"):
        if shutil.which(tool) is None:
            pytest.fail(f"M-004 requires host tool: {tool}")

    staging = tmp_path / "staging"
    staging.mkdir()
    original = b"A" * (3 * 4096)
    (staging / "m004-probe.txt").write_bytes(original)
    image = tmp_path / "probe.ext4"
    with image.open("wb") as stream:
        stream.truncate(32 * 1024 * 1024)
    # Explicit features, independent of the host's mke2fs.conf defaults.
    subprocess.run([
        "mkfs.ext4", "-q", "-F", "-b", "4096", "-I", "256",
        "-O", "none,extent,filetype,sparse_super,large_file,64bit",
        "-E", "lazy_itable_init=0", "-d", str(staging), str(image),
    ], check=True)
    profile = subprocess.run(
        ["dumpe2fs", "-h", str(image)], capture_output=True, text=True, check=True,
    ).stdout
    assert "has_journal" not in profile

    target = ROOT / "target/fs-write-probe"
    subprocess.run([
        "cargo", "build", "--features", "fs-write-probe", "--target-dir", str(target),
    ], cwd=ROOT, check=True)
    session = QemuSession(
        kernel=target / "riscv32ima-unknown-none-elf/debug/main",
        image=image,
        qemu=os.environ.get("QEMU_SYSTEM_RISCV32", "qemu-system-riscv32"),
    )
    try:
        session.start()
        session._wait_for(b"M004_WRITE_PROBE_OK", request.config.getoption("--qemu-boot-timeout"))
    except Exception:
        session.close()
        session.save_artifacts(ROOT / "build/test-artifacts", "m004-probe")
        raise
    finally:
        session.close()

    try:
        for name, expected in (
            ("m004-probe.txt", original[:8184] + b"M004-cross-block-write" + original[8184 + len(b"M004-cross-block-write"):]),
            ("m004-created.txt", b"Z" * (2 * 4096 + 37)),
        ):
            destination = tmp_path / name
            subprocess.run([
                "debugfs", "-R", f"dump /{name} {destination}", str(image),
            ], check=True, capture_output=True)
            assert destination.read_bytes() == expected
        check = subprocess.run(["e2fsck", "-fn", str(image)], capture_output=True, text=True)
        assert check.returncode == 0, check.stdout + check.stderr
    except Exception:
        session.save_artifacts(ROOT / "build/test-artifacts", "m004-verification")
        raise
