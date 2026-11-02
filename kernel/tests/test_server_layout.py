"""Validate the common server ELF contract without running a server runtime."""

import shutil
import struct
import subprocess

import pytest

from conftest import ROOT


@pytest.fixture
def link_server(tmp_path):
    clang = shutil.which("clang")
    linker = shutil.which("rust-lld")
    if not clang or not linker:
        pytest.skip("server layout validation needs clang and rust-lld")
    obj = tmp_path / "layout.o"
    subprocess.run([
        clang, "--target=riscv32-unknown-none-elf", "-march=rv32ima", "-mabi=ilp32",
        "-c", str(ROOT / "tests/user/server_layout.S"), "-o", str(obj),
    ], check=True)

    def link(*definitions):
        binary = tmp_path / "server.elf"
        result = subprocess.run([
            linker, "-flavor", "gnu", "-m", "elf32lriscv", "--no-relax",
            "-T", str(ROOT.parent / "serverlib/linker/server.ld"),
            *[f"--defsym={definition}" for definition in definitions],
            str(obj), "-o", str(binary),
        ], capture_output=True, text=True)
        return result, binary

    return link


def elf_layout(binary):
    data = binary.read_bytes()
    assert data[:6] == b"\x7fELF\x01\x01"  # ELF32, little endian
    header = struct.unpack_from("<16sHHIIIIIHHHHHH", data)
    entry, phoff, shoff = header[4:7]
    phsize, phnum, shsize, shnum = header[9:13]
    segments = [struct.unpack_from("<8I", data, phoff + i * phsize)
                for i in range(phnum)]
    sections = [struct.unpack_from("<10I", data, shoff + i * shsize)
                for i in range(shnum)]
    symbols = {}
    for section in sections:
        if section[1] != 2:  # SHT_SYMTAB
            continue
        strings = sections[section[6]]
        names = data[strings[4]:strings[4] + strings[5]]
        for offset in range(section[4], section[4] + section[5], section[9]):
            name, value = struct.unpack_from("<II", data, offset)
            symbols[names[name:names.index(b"\0", name)].decode()] = value
    return data, entry, segments, symbols


@pytest.mark.parametrize("load_address", [0x10000, 0x20000])
def test_fixed_heap_is_writable_noload_memory(link_server, load_address):
    layouts = []
    for size in (4096, 262144):
        definitions = [f"SERVER_HEAP_SIZE={size}"]
        if load_address != 0x10000:
            definitions.append(f"SERVER_LOAD_ADDRESS={load_address}")
        result, binary = link_server(*definitions)
        assert result.returncode == 0, result.stderr
        data, entry, segments, symbols = elf_layout(binary)
        assert entry == symbols["_start"] == load_address
        assert symbols["__server_image_start"] == load_address
        start, end = symbols["__server_heap_start"], symbols["__server_heap_end"]
        assert start % 4096 == end % 4096 == 0
        assert end - start == size
        assert symbols["__server_image_end"] == end
        assert symbols["__server_bss_end"] - symbols["__server_bss_start"] == 32
        assert symbols["__server_bss_end"] <= start
        assert "__global_pointer$" in symbols
        loads = [p for p in segments if p[0] == 1]
        assert [p[6] for p in loads] == [5, 4, 6]  # RX, R, RW
        writable = loads[-1]
        assert writable[2] + writable[4] <= start  # outside file-backed bytes
        assert writable[2] + writable[5] == end  # inside mapped memory
        layouts.append((len(data), writable[4], writable[5]))
    assert layouts[0][:2] == layouts[1][:2]
    assert layouts[1][2] - layouts[0][2] == 262144 - 4096


@pytest.mark.parametrize("definitions,diagnostic", [
    ([], "symbol not found: SERVER_HEAP_SIZE"),
    (["SERVER_HEAP_SIZE=0"], "SERVER_HEAP_SIZE must be nonzero"),
    (["SERVER_HEAP_SIZE=4097"], "SERVER_HEAP_SIZE must be page-aligned"),
    (["SERVER_HEAP_SIZE=4096", "SERVER_LOAD_ADDRESS=65537"],
     "server load address must be page-aligned"),
])
def test_invalid_server_layout_is_rejected(link_server, definitions, diagnostic):
    result, _ = link_server(*definitions)
    assert result.returncode != 0
    assert diagnostic in result.stderr
