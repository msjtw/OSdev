"""External host validation of the production terminal mechanism policy."""
import subprocess
from conftest import ROOT


def test_terminal_mechanism_policy(tmp_path):
    binary = tmp_path / "terminal"
    subprocess.run(["rustc", "--edition", "2024", "--test",
                    str(ROOT / "tests/host/terminal.rs"), "-o", str(binary)], check=True)
    subprocess.run([str(binary)], check=True)
