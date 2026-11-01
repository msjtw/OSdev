use core::arch::asm;

use crate::{Result, abi, error::decode};

pub unsafe fn syscall(number: usize, args: [usize; 6]) -> usize {
    let result;
    unsafe {
        asm!(
            "ecall",
            in("a7") number,
            inlateout("a0") args[0] => result,
            in("a1") args[1],
            in("a2") args[2],
            in("a3") args[3],
            in("a4") args[4],
            in("a5") args[5],
            options(nostack),
        );
    }
    result
}

pub fn read(fd: usize, destination: &mut [u8]) -> Result<usize> {
    decode(unsafe {
        syscall(
            abi::SYS_READ,
            [
                fd,
                destination.as_mut_ptr() as usize,
                destination.len(),
                0,
                0,
                0,
            ],
        )
    })
}

pub fn write(fd: usize, source: &[u8]) -> Result<usize> {
    decode(unsafe {
        syscall(
            abi::SYS_WRITE,
            [fd, source.as_ptr() as usize, source.len(), 0, 0, 0],
        )
    })
}

pub fn getpid() -> Result<usize> {
    decode(unsafe { syscall(abi::SYS_GETPID, [0; 6]) })
}

pub fn exit(status: i32) -> ! {
    unsafe { syscall(abi::SYS_EXIT, [status as usize, 0, 0, 0, 0, 0]) };
    loop {
        core::hint::spin_loop()
    }
}
