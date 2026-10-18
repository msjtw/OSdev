use core::ptr::write_volatile;

use crate::process::{Process, fd::Errno, signals};

const LINUX_REBOOT_MAGIC1: usize = 0xfee1_dead;
const LINUX_REBOOT_MAGIC2: [usize; 3] = [0x2812_1969, 0x0512_1996, 0x1604_1998];
const LINUX_REBOOT_CMD_POWER_OFF: usize = 0x4321_fedc;
const QEMU_VIRT_SYSCON: usize = 0x0010_0000;
const QEMU_POWER_OFF: u32 = 0x5555;
const SIGUSR2: usize = 12;

pub fn sys_sync(proc: &mut Process) {
    proc.trapframe.a0 = 0;
}


pub fn sys_kill(proc: &mut Process) {
    // BusyBox poweroff(8) in the supplied rootfs asks PID 1 to handle
    // SIGUSR2. There is no separate user-space init service yet, so retain
    // this QEMU lifecycle compatibility hook; all other kill calls below use
    // normal scoped signal delivery.
    if proc.trapframe.a0 == 1 && proc.trapframe.a1 == SIGUSR2 {
        power_off();
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
    if command != LINUX_REBOOT_CMD_POWER_OFF {
        proc.trapframe.a0 = Errno::EINVAL.into();
        return;
    }

    power_off();
}

fn power_off() -> ! {
    unsafe { write_volatile(QEMU_VIRT_SYSCON as *mut u32, QEMU_POWER_OFF) };
    loop {
        core::hint::spin_loop();
    }
}
