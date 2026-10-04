use alloc::vec::Vec;

const EI_NIDENT: usize = 16;

#[repr(C)]
#[derive(Debug)]
struct ElfEhdr {
    e_ident: [u8; EI_NIDENT],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u32,
    e_phoff: u32,
    e_shoff: u32,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}
#[repr(C)]
#[derive(Debug)]
pub struct Elf32_Phdr {
    pub p_type: u32,
    pub p_offset: u32, // Elf32_Off
    pub p_vaddr: u32,  // Elf32_Addr
    pub p_paddr: u32,  // Elf32_Addr
    pub p_filesz: u32,
    pub p_memsz: u32,
    pub p_flags: u32,
    pub p_align: u32,
}

#[repr(C)]
#[derive(Debug)]
pub struct Elf64_Phdr {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64, // Elf64_Off
    pub p_vaddr: u64,  // Elf64_Addr
    pub p_paddr: u64,  // Elf64_Addr
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

const PT_NULL: u32 = 0;
pub const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const PT_NOTE: u32 = 4;
const PT_SHLIB: u32 = 5;
const PT_PHDR: u32 = 6;
const PT_TLS: u32 = 7;
const PT_NUM: u32 = 8;

const PT_LOOS: u32 = 0x6000_0000;
const PT_GNU_EH_FRAME: u32 = 0x6474_e550;
const PT_GNU_STACK: u32 = 0x6474_e551;
const PT_GNU_RELRO: u32 = 0x6474_e552;

const PT_LOSUNW: u32 = 0x6fff_fffa;
const PT_SUNWBSS: u32 = 0x6fff_fffa;
const PT_SUNWSTACK: u32 = 0x6fff_fffb;
const PT_HISUNW: u32 = 0x6fff_ffff;

const PT_HIOS: u32 = 0x6fff_ffff;

const PT_LOPROC: u32 = 0x7000_0000;
const PT_HIPROC: u32 = 0x7fff_ffff;

pub const PF_X: u32 = 1;
pub const PF_W: u32 = 2;
pub const PF_R: u32 = 4;

pub fn get_elf_segments(img: &[u8]) -> Result<Vec<Elf32_Phdr>, ()> {
    let ehdr = unsafe { core::ptr::read_unaligned(img.as_ptr() as *const ElfEhdr) };
    if ehdr.e_ident[0..4] != [0x7f, 'E' as u8, 'L' as u8, 'F' as u8] {
        return Err(());
    }
    if ehdr.e_ident[4] != 1 {
        // not 32 bit elf
        return Err(());
    }
    // println!("{:x?}", ehdr);

    Ok((0..ehdr.e_phnum)
        .map(|i| {
            let offset = ehdr.e_phoff as usize + ehdr.e_phentsize as usize * i as usize;
            let phdr_addr = unsafe { img.as_ptr().add(offset) };
            unsafe { core::ptr::read_unaligned(phdr_addr as *const Elf32_Phdr) }
        })
        .collect())
}

pub fn segment_bytes<'a>(elf: &'a [u8], phdr: &Elf32_Phdr) -> Option<&'a [u8]> {
    let start = phdr.p_offset as usize;
    let size = phdr.p_filesz as usize;
    let end = start.checked_add(size)?;

    elf.get(start..end)
}
