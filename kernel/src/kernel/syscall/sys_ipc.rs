use crate::{
    errno::Errno,
    process::{
        Process,
        ipc::{self, abi::Message},
    },
    virtmemory::copy_in,
};

pub fn sys_ipc_call(proc: &mut Process) {
    let destination = proc.trapframe.a0;
    let result = copy_in::<Message>(&mut proc.pagetable, proc.trapframe.a1)
        .map_err(|_| Errno::EFAULT)
        .and_then(|message| ipc::user_call(proc, destination, message));
    proc.trapframe.a0 = match result {
        Ok(result) => result,
        Err(error) => error.into(),
    };
}

pub fn sys_ipc_receive(proc: &mut Process) {
    proc.trapframe.a0 = match ipc::serv_receive(proc, proc.trapframe.a0) {
        Ok(id) => id,
        Err(error) => error.into(),
    };
}

pub fn sys_ipc_cancel(proc: &mut Process) {
    let mut kernel = crate::KERNEL.get().unwrap().lock();
    proc.trapframe.a0 = match ipc::cancel_outgoing(&mut kernel, proc, proc.trapframe.a0) {
        Ok(()) => 0,
        Err(error) => error.into(),
    };
}

pub fn sys_ipc_submit(proc: &mut Process) {
    let destination = proc.trapframe.a0;
    let result = copy_in::<Message>(&mut proc.pagetable, proc.trapframe.a1)
        .map_err(|_| Errno::EFAULT)
        .and_then(|message| ipc::user_submit(proc, destination, message));
    proc.trapframe.a0 = match result {
        Ok(id) => id,
        Err(error) => error.into(),
    };
}

pub fn sys_ipc_receive_event(proc: &mut Process) {
    proc.trapframe.a0 = match ipc::serv_receive_event(proc, proc.trapframe.a0) {
        Ok(()) => 0,
        Err(error) => error.into(),
    };
}

pub fn sys_ipc_reply(proc: &mut Process) {
    proc.trapframe.a0 = match ipc::serv_reply(
        proc,
        proc.trapframe.a0,
        proc.trapframe.a1,
        proc.trapframe.a2,
        proc.trapframe.a3,
    ) {
        Ok(()) => 0,
        Err(error) => error.into(),
    };
}

pub fn sys_ipc_read_payload(proc: &mut Process) {
    proc.trapframe.a0 = match ipc::serv_read_payload(
        proc,
        proc.trapframe.a0,
        proc.trapframe.a1,
        proc.trapframe.a2,
        proc.trapframe.a3,
    ) {
        Ok(length) => length,
        Err(error) => error.into(),
    };
}
