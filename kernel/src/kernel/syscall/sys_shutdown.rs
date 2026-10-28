use core::ptr::write_volatile;

use crate::{
    drivers::block,
    errno::Errno,
    process::{Process, signals},
};

const LINUX_REBOOT_MAGIC1: usize = 0xfee1_dead;
const LINUX_REBOOT_MAGIC2: [usize; 3] = [0x2812_1969, 0x0512_1996, 0x1604_1998];
const LINUX_REBOOT_CMD_RESTART: usize = 0x0123_4567;
const LINUX_REBOOT_CMD_HALT: usize = 0xcdef_0123;
const LINUX_REBOOT_CMD_POWER_OFF: usize = 0x4321_fedc;
const QEMU_VIRT_SYSCON: usize = 0x0010_0000;
const QEMU_POWER_OFF: u32 = 0x5555;
const QEMU_REBOOT: u32 = 0x7777;
const SIGUSR2: usize = 12;

pub fn sys_sync(proc: &mut Process) {
    proc.trapframe.a0 = match block::flush_all() {
        Ok(()) => 0,
        Err(()) => Errno::EIO.into(),
    };
}


pub fn sys_kill(proc: &mut Process) {
    // BusyBox poweroff(8) in the supplied rootfs asks PID 1 to handle
    // SIGUSR2. There is no separate user-space init service yet, so retain
    // this QEMU lifecycle compatibility hook; all other kill calls below use
    // normal scoped signal delivery.
    if proc.trapframe.a0 == 1 && proc.trapframe.a1 == SIGUSR2 {
        shutdown(proc, QEMU_POWER_OFF);
        return;
    }
    let result = signals::send(proc, proc.trapframe.a0 as isize, proc.trapframe.a1);
    proc.trapframe.a0 = match result {
        Ok(()) => 0,
        Err(error) => error.into(),
    };
}

pub fn sys_reboot(proc: &mut Process) {
    let magic1 = proc.trapframe.a0;
    let magic2 = proc.trapframe.a1;
    let command = proc.trapframe.a2;
    if magic1 != LINUX_REBOOT_MAGIC1 || !LINUX_REBOOT_MAGIC2.contains(&magic2) {
        proc.trapframe.a0 = Errno::EINVAL.into();
        return;
    }
    match command {
        LINUX_REBOOT_CMD_RESTART => shutdown(proc, QEMU_REBOOT),
        LINUX_REBOOT_CMD_HALT | LINUX_REBOOT_CMD_POWER_OFF => shutdown(proc, QEMU_POWER_OFF),
        _ => proc.trapframe.a0 = Errno::EINVAL.into(),
    }
}

fn shutdown(proc: &mut Process, command: u32) {
    if block::flush_all().is_err() {
        proc.trapframe.a0 = Errno::EIO.into();
        return;
    }
    unsafe { write_volatile(QEMU_VIRT_SYSCON as *mut u32, command) };
    loop {
        core::hint::spin_loop();
    }
}
