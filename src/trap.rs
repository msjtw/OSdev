pub mod trampoline;

use core::arch::naked_asm;

use alloc::format;

use crate::{CPU, kernel::syscall::syscall, print, process::prepare_return, read_csr, write_csr};

const SIE_SEIE: usize = 1 << 9;
const SIE_STIE: usize = 1 << 5;
const SSTATUS_SIE: usize = 1 << 1;

pub fn init_trap() {
    unsafe {
        write_csr!(stvec, kernelvec as *const () as u32);

        // NOTE: Request first timer interrupt.
        // The sstc extension was enabled by openSBI

        // Enable timer and external interrupts
        // let time = read_csr!(time);
        // write_csr!(stimecmp, time + 1_00_000);

        let sie = read_csr!(sie);
        print!("sie: {:b}\n", sie);
        write_csr!(sie, sie | SIE_SEIE | SIE_STIE);
        let sie = read_csr!(sie);
        print!("sie: {:b}\n", sie);

        // let sstatus = read_csr!(sstatus);
        // print!("sstatus: {:b}\n", sstatus);
        // write_csr!(sstatus, sstatus | 0b10);
        // let sstatus = read_csr!(sstatus);
        // print!("sstatus: {:b}\n", sstatus);aa
    }
}

pub unsafe fn interrupt_on() {
    unsafe {
        let sstatus = read_csr!(sstatus);
        write_csr!(sstatus, sstatus | SSTATUS_SIE);
    }
}

pub unsafe fn interrupt_off() {
    unsafe {
        let sstatus = read_csr!(sstatus);
        write_csr!(sstatus, sstatus & !SSTATUS_SIE);
    }
}

#[unsafe(naked)]
pub extern "C" fn kernelvec() {
    naked_asm!(
        "
        .align 4
        # make room to save registers.
        addi sp, sp, -128

        # save caller-saved registers.
        sw ra, 0(sp)
        # sw sp, 4(sp)
        sw gp, 8(sp)
        sw tp, 12(sp)
        sw t0, 16(sp)
        sw t1, 20(sp)
        sw t2, 24(sp)
        sw a0, 36(sp)
        sw a1, 40(sp)
        sw a2, 44(sp)
        sw a3, 48(sp)
        sw a4, 52(sp)
        sw a5, 56(sp)
        sw a6, 60(sp)
        sw a7, 64(sp)
        sw t3, 108(sp)
        sw t4, 112(sp)
        sw t5, 116(sp)
        sw t6, 120(sp)

        # call the C trap handler in trap.c
        call kerneltrap

        # restore registers.
        lw ra, 0(sp)
        # lw sp, 4(sp)
        lw gp, 8(sp)
        # not tp (contains hartid), in case we moved CPUs
        lw t0, 16(sp)
        lw t1, 20(sp)
        lw t2, 24(sp)
        lw a0, 36(sp)
        lw a1, 40(sp)
        lw a2, 44(sp)
        lw a3, 48(sp)
        lw a4, 52(sp)
        lw a5, 56(sp)
        lw a6, 60(sp)
        lw a7, 64(sp)
        lw t3, 108(sp)
        lw t4, 112(sp)
        lw t5, 116(sp)
        lw t6, 120(sp)

        addi sp, sp, 128

        # return to whatever we were doing in the kernel.
        sret
        "
    );
}
#[unsafe(no_mangle)]
extern "C" fn kerneltrap() {
    unsafe {
        let sepc = read_csr!(sepc);
        let sstatus = read_csr!(sstatus);
        let scause = read_csr!(scause);
        let stval = read_csr!(stval);

        print!(
            ">TRAP sepc=0x{:08x} sstatus=0b{:b} scause=0x{:x} stval=0x{:x}\n",
            sepc, sstatus, scause, stval,
        );

        // Because trap originated in kernel it coudl (what?)
        match scause {
            0x80000005 => {
                let time = read_csr!(time);
                print!(
                    ">time: 0x{:x}, next timer on: 0x{:x}\n",
                    time,
                    time + 1000000
                );
                write_csr!(stimecmp, time + 1000000);
                if !CPU.current.is_null() {
                    (*CPU.current).yeld();
                }
            }
            _ => panic!(),
        }

        write_csr!(sepc, sepc);
        write_csr!(sstatus, sstatus);
    }
}

pub extern "C" fn usertrap() -> u32 {
    let proc;
    unsafe {
        let sepc = read_csr!(sepc);
        let sstatus = read_csr!(sstatus);
        let scause = read_csr!(scause);
        proc = &mut (*CPU.current);

        print!(
            "user>TRAP sepc=0x{:08x} sstatus=0b{:b} scause=0x{:x}\n",
            sepc, sstatus, scause
        );

        // switch to kernel trap
        let kernelvec = kernelvec as *const () as u32;
        write_csr!(stvec, kernelvec);

        proc.trapframe.epc = read_csr!(sepc) as u32;

        match scause {
            8 => {
                // syscall
                proc.trapframe.epc += 4;
                interrupt_on();
                syscall();
            }
            0x80000005 => {
                let time = read_csr!(time);
                print!(
                    "user>time: 0x{:x}, next timer on: 0x{:x}\n",
                    time,
                    time + 1000000
                );
                write_csr!(stimecmp, time + 1000000);
                if !CPU.current.is_null() {
                    // NOTE: I dont think it's possible for it to be null
                    (*CPU.current).yeld();
                }
            }
            _ => panic!(),
        }
        prepare_return(proc);
    }
    let satp = proc.pagetable.unwrap().get_satp();
    return satp.into();
}
