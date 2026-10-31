use alloc::vec::Vec;

use crate::{
    KERNEL,
    errno::Errno,
    kernel::{
        Kernel,
        syscall::{SYS_READ, SYS_WRITE},
    },
    process::{ProcState, Process, signals},
    virtmemory::{copy_between, copy_in_slice, copy_out, copy_out_cont},
};

use super::{
    abi::{CUSTOM_BIT, KERNEL_SENDER_ID, Message},
    state::{CurrentReq, Request, RequestState, next_request_id},
};

fn alive(proc: &Process, pid: usize) -> bool {
    proc.pid == Some(pid) && !matches!(proc.state, ProcState::Unused | ProcState::Zombie)
}

fn channel(proc: &Process) -> usize {
    &proc.ipc as *const super::IpcQueue as usize
}

// wakeup single proc
fn wakeup(proc: &mut Process) {
    let _guard = proc.lock.lock();
    if proc.state == ProcState::Sleeping && proc.sleep_channel == Some(channel(proc)) {
        proc.state = ProcState::Runnable;
    }
}

pub fn kernel_send(
    kernel: &mut Kernel,
    destination: usize,
    message: Message,
    data: Option<Vec<u8>>,
) -> Result<Option<usize>, Errno> {
    let Kernel {
        process_table,
        ipc_req_id,
        ..
    } = kernel;
    let Some(server) = process_table
        .iter_mut()
        .find(|proc| alive(proc, destination))
    else {
        crate::log::debug!("ipc: dropping kernel message to absent pid {}", destination);
        return Ok(None);
    };
    let id = server.ipc.push(ipc_req_id, KERNEL_SENDER_ID, message, data)?;
    wakeup(server);
    Ok(Some(id))
}

/// Return the owned buffer together with the result, releasing its inbox entry.
pub fn kernel_read_reply(
    kernel: &mut Kernel,
    destination: usize,
    id: usize,
) -> Option<(usize, Option<Vec<u8>>)> {
    let server = kernel
        .process_table
        .iter_mut()
        .find(|proc| alive(proc, destination))?;
    let request = server.ipc.get(id)?;
    if request.message.sender != KERNEL_SENDER_ID || request.state != RequestState::Completed {
        return None;
    }
    let result = request.result?;
    let request = server.ipc.remove(id)?;
    Some((result, request.kernel_data))
}

// cancel current call, by removing it from inbox
pub fn cancel_call(kernel: &mut Kernel, proc: &mut Process) {
    let Some(current) = proc.ipc.pending_call else {
        return;
    };
    if proc.pid == Some(current.destination) {
        proc.ipc.remove(current.id);
    } else if let Some(server) = kernel
        .process_table
        .iter_mut()
        .find(|candidate| candidate.pid == Some(current.destination))
    {
        server.ipc.remove(current.id);
    }
    proc.ipc.finish_call(current.destination, current.id);
}

// checks whether a server’s request is still valid
pub(super) fn current_request<'a>(
    kernel: &Kernel,
    server: &'a Process,
    id: usize,
) -> Result<&'a Request, Errno> {
    let request = server.ipc.get(id).ok_or(Errno::ESRCH)?;
    if request.state != RequestState::Delivered {
        return Err(Errno::ESRCH);
    }
    if request.message.sender != KERNEL_SENDER_ID {
        let destination = server.pid.ok_or(Errno::ESRCH)?;
        let active = kernel.process_table.iter().any(|caller| {
            alive(caller, request.message.sender)
                && caller.ipc.pending_call == Some(CurrentReq { destination, id })
                && caller.ipc.call_result.is_none()
        });
        if !active {
            return Err(Errno::ESRCH);
        }
    }
    Ok(request)
}

// blocking send + read reply
pub fn user_call(proc: &mut Process, destination: usize, message: Message) -> Result<usize, Errno> {
    let sender = proc.pid.ok_or(Errno::ESRCH)?;
    {
        let mut kernel = KERNEL.get().unwrap().lock();
        if proc.ipc.pending_call.is_some() {
            return Err(Errno::EAGAIN);
        }
        if signals::has_interrupting_pending(proc) {
            return Err(Errno::EINTR);
        }
        let id = if destination == sender {
            proc.ipc.push(&mut kernel.ipc_req_id, sender, message, None)?
        } else {
            let Kernel {
                process_table,
                ipc_req_id,
                ..
            } = &mut *kernel;
            if let Some(server) = process_table
                .iter_mut()
                .find(|candidate| alive(candidate, destination))
            {
                let id = server.ipc.push(ipc_req_id, sender, message, None)?;
                wakeup(server);
                id
            } else {
                crate::log::debug!("ipc: calling non existing process {}", destination);
                next_request_id(ipc_req_id)?
            }
        };
        proc.ipc.begin_call(destination, id)?;
    }

    loop {
        let mut kernel = KERNEL.get().unwrap().lock();
        let current = proc
            .ipc
            .pending_call
            .expect("IPC call missing current request");
        if let Some(result) = proc.ipc.call_result.take() {
            proc.ipc.finish_call(current.destination, current.id);
            return Ok(result);
        }
        if signals::has_interrupting_pending(proc) {
            cancel_call(&mut kernel, proc);
            return Err(Errno::EINTR);
        }

        unsafe { proc.lock.lock_manual() };
        proc.sleep_channel = Some(channel(proc));
        proc.state = ProcState::Sleeping;
        drop(kernel);
        unsafe { proc.sleep_locked() };
    }
}

pub fn serv_receive(proc: &mut Process, message_out: usize) -> Result<usize, Errno> {
    loop {
        let kernel = KERNEL.get().unwrap().lock();
        if signals::has_interrupting_pending(proc) {
            return Err(Errno::EINTR);
        }
        if let Some((id, message)) = proc.ipc.peek().map(|request| (request.id, request.message)) {
            copy_out(&mut proc.pagetable, message_out, message).map_err(|_| Errno::EFAULT)?;
            proc.ipc.receive();
            return Ok(id);
        }

        unsafe { proc.lock.lock_manual() };
        proc.sleep_channel = Some(channel(proc));
        proc.state = ProcState::Sleeping;
        drop(kernel);
        unsafe { proc.sleep_locked() };
    }
}

fn payload_range(message: Message, offset: usize, length: usize) -> Result<usize, Errno> {
    let end = offset.checked_add(length).ok_or(Errno::EINVAL)?;
    if end > message.args[2] {
        return Err(Errno::EINVAL);
    }
    message.args[1].checked_add(offset).ok_or(Errno::EFAULT)
}

pub fn serv_read_payload(
    proc: &mut Process,
    id: usize,
    offset: usize,
    destination: usize,
    length: usize,
) -> Result<usize, Errno> {
    let mut kernel = KERNEL.get().unwrap().lock();
    let message = current_request(&kernel, proc, id)?.message;
    if message.operation != SYS_WRITE && message.operation & CUSTOM_BIT == 0 {
        return Err(Errno::EINVAL);
    }
    let source = payload_range(message, offset, length)?;
    if message.sender == KERNEL_SENDER_ID {
        let request = proc.ipc.get(id).unwrap();
        let data = request.kernel_data.as_ref().ok_or(Errno::EINVAL)?;
        let bytes = data.get(offset..offset + length).ok_or(Errno::EINVAL)?;
        copy_out_cont(&mut proc.pagetable, destination, bytes).map_err(|_| Errno::EFAULT)?;
    } else {
        let caller = kernel
            .process_table
            .iter_mut()
            .find(|caller| alive(caller, message.sender))
            .ok_or(Errno::ESRCH)?;
        copy_between(
            &mut caller.pagetable,
            source,
            &mut proc.pagetable,
            destination,
            length,
        )
        .map_err(|_| Errno::EFAULT)?;
    }
    Ok(length)
}

pub fn serv_reply(
    proc: &mut Process,
    id: usize,
    result: usize,
    source: usize,
    length: usize,
) -> Result<(), Errno> {
    let mut kernel = KERNEL.get().unwrap().lock();
    let Some(request) = proc.ipc.get(id) else {
        // stale reply
        return Ok(());
    };
    if request.state == RequestState::Cancelled {
        proc.ipc.remove(id);
        return Ok(());
    }
    if request.state == RequestState::Completed {
        return Ok(());
    }
    if request.state != RequestState::Delivered {
        return Err(Errno::EINVAL);
    }
    let message = match current_request(&kernel, proc, id) {
        Ok(request) => request.message,
        Err(_) => {
            proc.ipc.remove(id);
            return Ok(());
        }
    };
    if length != 0 {
        if (result as isize) < 0
            || (message.operation != SYS_READ && message.operation & CUSTOM_BIT == 0)
        {
            return Err(Errno::EINVAL);
        }
        let destination = payload_range(message, 0, length)?;
        if message.sender == KERNEL_SENDER_ID {
            let request = proc.ipc.get_mut(id).unwrap();
            let data = request.kernel_data.as_mut().ok_or(Errno::EINVAL)?;
            let bytes = data.get_mut(..length).ok_or(Errno::EINVAL)?;
            copy_in_slice(&mut proc.pagetable, source, bytes).map_err(|_| Errno::EFAULT)?;
        } else {
            let caller = kernel
                .process_table
                .iter_mut()
                .find(|caller| alive(caller, message.sender))
                .ok_or(Errno::ESRCH)?;
            copy_between(
                &mut proc.pagetable,
                source,
                &mut caller.pagetable,
                destination,
                length,
            )
            .map_err(|_| Errno::EFAULT)?;
        }
    }
    if message.sender == KERNEL_SENDER_ID {
        let request = proc.ipc.get_mut(id).unwrap();
        request.result = Some(result);
        request.state = RequestState::Completed;
        return Ok(());
    }

    let destination = proc.pid.ok_or(Errno::ESRCH)?;
    if let Some(caller) = kernel
        .process_table
        .iter_mut()
        .find(|candidate| alive(candidate, message.sender))
    {
        if caller.ipc.pending_call == Some(CurrentReq { destination, id }) {
            caller.ipc.call_result = Some(result);
            wakeup(caller);
        }
    }
    proc.ipc.remove(id);
    Ok(())
}
