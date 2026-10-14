use crate::{
    process::{Process, fd::Errno},
    virtmemory::{PAGESIZE, PTE_R, PTE_W, PTE_X, VmError},
};

// brk() sets the end of the data segment to the value specified by addr.
pub fn sys_brk(proc: &mut Process) {
    let addr = proc.trapframe.a0;
    proc.trapframe.a0 = proc.pagetable.brk(addr);
}

// mmap() creates a new mapping in the virtual address space of the calling process.
pub fn sys_mmap(proc: &mut Process) {
    const PROT_READ: usize = 0x1;
    const PROT_WRITE: usize = 0x2;
    const PROT_EXEC: usize = 0x4;
    const MAP_PRIVATE: usize = 0x02;
    const MAP_FIXED: usize = 0x10;
    const MAP_ANONYMOUS: usize = 0x20;

    let addr = proc.trapframe.a0;
    let length = proc.trapframe.a1;
    let prot = proc.trapframe.a2;
    let flags = proc.trapframe.a3;
    let fd = proc.trapframe.a4;
    let offset = proc.trapframe.a5;

    if length == 0
        || flags & (MAP_PRIVATE | MAP_ANONYMOUS) != (MAP_PRIVATE | MAP_ANONYMOUS)
        || fd != usize::MAX
        || offset != 0
        || prot & !(PROT_READ | PROT_WRITE | PROT_EXEC) != 0
    {
        proc.trapframe.a0 = Errno::EINVAL.into();
        return;
    }

    // Musl reserves a guard page at the current brk with an anonymous,
    // MAP_FIXED, PROT_NONE request. Sv32 cannot represent a user leaf PTE
    // with no R/W/X bits.
    if flags & MAP_FIXED != 0 {
        if prot == 0 && addr.is_multiple_of(PAGESIZE) {
            proc.trapframe.a0 = addr;
        } else {
            proc.trapframe.a0 = Errno::EINVAL.into();
        }
        return;
    }
    let mut perm = 0;
    if prot & PROT_READ != 0 {
        perm |= PTE_R;
    }
    if prot & PROT_WRITE != 0 {
        perm |= PTE_R | PTE_W;
    }
    if prot & PROT_EXEC != 0 {
        perm |= PTE_X;
    }
    proc.trapframe.a0 = match proc.pagetable.map_anonymous(addr, length, perm) {
        Ok(addr) => addr,
        Err(VmError::Invalid) => Errno::EINVAL.into(),
        Err(VmError::NoMemory) => Errno::ENOMEM.into(),
    };
}

// The munmap() system call deletes the mappings for the specified address range.
pub fn sys_munmap(proc: &mut Process) {
    let addr = proc.trapframe.a0;
    let length = proc.trapframe.a1;
    proc.trapframe.a0 = match proc.pagetable.unmap(addr, length) {
        Ok(()) => 0,
        Err(_) => Errno::EINVAL.into(),
    };
}
