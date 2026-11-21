use alloc::vec::Vec;

use crate::{
    KERNEL,
    errno::Errno,
    kernel::{
        Kernel,
        syscall::{SYS_READ, SYS_WRITE},
    },
    process::{ProcState, Process, signals},
    virtmemory::{copy_between, copy_in_cont, copy_in_slice, copy_out, copy_out_cont},
};

use super::{
    abi::{CUSTOM_BIT, KERNEL_SENDER_ID, Message},
    state::{CurrentReq, Request, RequestState, next_request_id},
};

/// Find a process by PID, excluding unused slots and zombies.
fn find_process(table: &mut [Process], pid: usize) -> Option<&mut Process> {
    table.iter_mut().find(|proc| {
        proc.pid == Some(pid) && !matches!(proc.state, ProcState::Unused | ProcState::Zombie)
    })
}

/// Identify the stable sleep channel used for this process's IPC events.
fn channel(proc: &Process) -> usize {
    &proc.ipc as *const super::IpcState as usize
}

/// Enqueue a kernel-originated request, consuming its payload even on failure.
pub fn kernel_send(
    kernel: &mut Kernel,
    destination: usize,
    message: Message,
    mut data: Option<Vec<u8>>,
) -> Result<Option<usize>, Errno> {
    kernel_send_retained(kernel, destination, message, &mut data)
}

/// Enqueue kernel work while retaining its payload if the destination is absent or enqueue fails.
pub fn kernel_send_retained(
    kernel: &mut Kernel,
    destination: usize,
    message: Message,
    data: &mut Option<Vec<u8>>,
) -> Result<Option<usize>, Errno> {
    let Kernel {
        process_table,
        ipc_req_id,
        ..
    } = kernel;
    let Some(server) = find_process(process_table, destination) else {
        crate::log::debug!("ipc: dropping kernel message to absent pid {}", destination);
        return Ok(None);
    };
    let id = server
        .ipc
        .inbox
        .push_retained(ipc_req_id, KERNEL_SENDER_ID, message, data)?;
    server.wakeup_on(Some(channel(server)));
    Ok(Some(id))
}

/// Perform an interruptible kernel-originated call with an owned payload buffer.
// Like user_call, returns the server's raw result word without interpreting it.
pub fn kernel_call(
    proc: &mut Process,
    destination: usize,
    message: Message,
    data: Option<Vec<u8>>,
) -> Result<usize, Errno> {
    {
        let mut kernel = KERNEL.get().unwrap().lock();
        if proc.ipc.pending_call.is_some() {
            return Err(Errno::EAGAIN);
        }
        if signals::has_interrupting_pending(proc) {
            return Err(Errno::EINTR);
        }
        let id = match kernel_send(&mut kernel, destination, message, data)? {
            Some(id) => id,
            None => next_request_id(&mut kernel.ipc_req_id)?,
        };
        proc.ipc.begin_call(destination, id)?;
    }
    loop {
        let mut kernel = KERNEL.get().unwrap().lock();
        let current = proc
            .ipc
            .pending_call
            .expect("kernel IPC call missing request");
        if let Some((result, _data)) = kernel_read_reply(&mut kernel, destination, current.id) {
            proc.ipc.finish_call(destination, current.id);
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

/// Collect a completed kernel request's result and owned buffer from its destination.
pub fn kernel_read_reply(
    kernel: &mut Kernel,
    destination: usize,
    id: usize,
) -> Option<(usize, Option<Vec<u8>>)> {
    let server = find_process(&mut kernel.process_table, destination)?;
    let request = server.ipc.inbox.get(id)?;
    if request.message.sender != KERNEL_SENDER_ID || request.state != RequestState::Completed {
        return None;
    }
    let result = request.result?;
    let request = server.ipc.inbox.remove(id)?;
    Some((result, request.kernel_data))
}

/// Cancel the current blocking call and revoke its destination request.
pub fn cancel_call(kernel: &mut Kernel, proc: &mut Process) {
    let Some(current) = proc.ipc.pending_call else {
        return;
    };
    remove_destination_request(kernel, proc, current);
    proc.ipc.finish_call(current.destination, current.id);
}

/// Remove a destination entry, including one retained in a zombie's inbox.
fn remove_destination_request(kernel: &mut Kernel, proc: &mut Process, current: CurrentReq) {
    if proc.pid == Some(current.destination) {
        proc.ipc.inbox.remove(current.id);
    } else if let Some(server) = kernel
        .process_table
        .iter_mut()
        .find(|candidate| candidate.pid == Some(current.destination))
    {
        server.ipc.inbox.remove(current.id);
    }
}

/// Revoke one caller-owned outbox entry and discard any unconsumed completion.
pub fn cancel_outgoing(kernel: &mut Kernel, proc: &mut Process, id: usize) -> Result<(), Errno> {
    let current = proc.ipc.outbox.remove(id).ok_or(Errno::ESRCH)?;
    remove_destination_request(kernel, proc, current);
    Ok(())
}

/// Revoke every outbox entry without allocating during exit or exec.
pub fn cancel_outbox(kernel: &mut Kernel, proc: &mut Process) {
    while let Some(current) = proc.ipc.outbox.pop() {
        remove_destination_request(kernel, proc, current);
    }
}

/// Validate that a delivered request still has a live caller operation.
pub(super) fn current_request<'a>(
    kernel: &mut Kernel,
    server: &'a Process,
    id: usize,
) -> Result<&'a Request, Errno> {
    let request = server.ipc.inbox.get(id).ok_or(Errno::ESRCH)?;
    if request.state != RequestState::Delivered {
        return Err(Errno::ESRCH);
    }
    if request.message.sender != KERNEL_SENDER_ID {
        let destination = server.pid.ok_or(Errno::ESRCH)?;
        let is_active =
            |caller: &Process| caller.ipc.outgoing_active(CurrentReq { destination, id });
        let active = if request.message.sender == destination {
            is_active(server)
        } else {
            find_process(&mut kernel.process_table, request.message.sender)
                .is_some_and(|p| is_active(p))
        };
        if !active {
            return Err(Errno::ESRCH);
        }
    }
    Ok(request)
}

/// Enqueue a user request with its allocated ID and wake any live destination.
fn enqueue_user_request(
    kernel: &mut Kernel,
    proc: &mut Process,
    destination: usize,
    id: usize,
    message: Message,
) -> Result<(), Errno> {
    let sender = proc.pid.ok_or(Errno::ESRCH)?;
    if destination == sender {
        proc.ipc.inbox.push_with_id(id, sender, message, &mut None)
    } else {
        if let Some(server) = find_process(&mut kernel.process_table, destination) {
            server
                .ipc
                .inbox
                .push_with_id(id, sender, message, &mut None)?;
            server.wakeup_on(Some(channel(server)));
        }
        Ok(())
    }
}

/// Send a user request and wait interruptibly for its raw reply result.
pub fn user_call(proc: &mut Process, destination: usize, message: Message) -> Result<usize, Errno> {
    {
        let mut kernel = KERNEL.get().unwrap().lock();
        if proc.ipc.pending_call.is_some() {
            return Err(Errno::EAGAIN);
        }
        if signals::has_interrupting_pending(proc) {
            return Err(Errno::EINTR);
        }
        let id = next_request_id(&mut kernel.ipc_req_id)?;
        enqueue_user_request(&mut kernel, proc, destination, id, message)?;
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

/// Wait interruptibly for a request and mark it delivered only after successful copy-out.
pub fn serv_receive(proc: &mut Process, message_out: usize) -> Result<usize, Errno> {
    loop {
        let kernel = KERNEL.get().unwrap().lock();
        if signals::has_interrupting_pending(proc) {
            return Err(Errno::EINTR);
        }
        if let Some((id, message)) = proc
            .ipc
            .inbox
            .peek()
            .map(|request| (request.id, request.message))
        {
            copy_out(&mut proc.pagetable, message_out, message).map_err(|_| Errno::EFAULT)?;
            proc.ipc.inbox.receive();
            return Ok(id);
        }

        unsafe { proc.lock.lock_manual() };
        proc.sleep_channel = Some(channel(proc));
        proc.state = ProcState::Sleeping;
        drop(kernel);
        unsafe { proc.sleep_locked() };
    }
}

/// Submit user work without waiting, reserving completion storage before publishing it.
pub fn user_submit(
    proc: &mut Process,
    destination: usize,
    message: Message,
) -> Result<usize, Errno> {
    let mut kernel = KERNEL.get().unwrap().lock();
    if signals::has_interrupting_pending(proc) {
        return Err(Errno::EINTR);
    }
    let id = next_request_id(&mut kernel.ipc_req_id)?;
    proc.ipc.outbox.push(destination, id)?;
    if let Err(error) = enqueue_user_request(&mut kernel, proc, destination, id, message) {
        proc.ipc.outbox.remove(id);
        return Err(error);
    }
    Ok(id)
}

/// Wait for a request or completion without consuming it on interruption or copy failure.
pub fn serv_receive_event(proc: &mut Process, event_out: usize) -> Result<(), Errno> {
    loop {
        let kernel = KERNEL.get().unwrap().lock();
        if signals::has_interrupting_pending(proc) {
            return Err(Errno::EINTR);
        }
        if let Some(event) = proc.ipc.peek_event() {
            copy_out(&mut proc.pagetable, event_out, event).map_err(|_| Errno::EFAULT)?;
            proc.ipc.consume_event(event);
            return Ok(());
        }
        unsafe { proc.lock.lock_manual() };
        proc.sleep_channel = Some(channel(proc));
        proc.state = ProcState::Sleeping;
        drop(kernel);
        unsafe { proc.sleep_locked() };
    }
}

/// Validate payload bounds and compute the caller address for a requested slice.
fn payload_range(message: Message, offset: usize, length: usize) -> Result<usize, Errno> {
    let end = offset.checked_add(length).ok_or(Errno::EINVAL)?;
    if end > message.args[2] {
        return Err(Errno::EINVAL);
    }
    message.args[1].checked_add(offset).ok_or(Errno::EFAULT)
}

/// Copy a live request's source bytes into the receiving process's buffer.
pub fn serv_read_payload(
    proc: &mut Process,
    id: usize,
    offset: usize,
    destination: usize,
    length: usize,
) -> Result<usize, Errno> {
    let mut kernel = KERNEL.get().unwrap().lock();
    let message = current_request(&mut kernel, proc, id)?.message;
    if message.operation != SYS_WRITE && message.operation & CUSTOM_BIT == 0 {
        return Err(Errno::EINVAL);
    }
    let source = payload_range(message, offset, length)?;
    if message.sender == KERNEL_SENDER_ID {
        let request = proc.ipc.inbox.get(id).unwrap();
        let data = request.kernel_data.as_ref().ok_or(Errno::EINVAL)?;
        let bytes = data.get(offset..offset + length).ok_or(Errno::EINVAL)?;
        copy_out_cont(&mut proc.pagetable, destination, bytes).map_err(|_| Errno::EFAULT)?;
    } else if proc.pid == Some(message.sender) {
        let bytes =
            copy_in_cont::<u8>(&mut proc.pagetable, source, length).map_err(|_| Errno::EFAULT)?;
        copy_out_cont(&mut proc.pagetable, destination, &bytes).map_err(|_| Errno::EFAULT)?;
    } else {
        let caller = find_process(&mut kernel.process_table, message.sender).ok_or(Errno::ESRCH)?;
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

/// Copy optional reply bytes and complete a live request, silently ignoring stale replies.
pub fn serv_reply(
    proc: &mut Process,
    id: usize,
    result: usize,
    source: usize,
    length: usize,
) -> Result<(), Errno> {
    let mut kernel = KERNEL.get().unwrap().lock();
    let Some(request) = proc.ipc.inbox.get(id) else {
        // stale reply
        return Ok(());
    };
    if request.state == RequestState::Completed {
        return Ok(());
    }
    if request.state != RequestState::Delivered {
        return Err(Errno::EINVAL);
    }
    let message = match current_request(&mut kernel, proc, id) {
        Ok(request) => request.message,
        Err(_) => {
            proc.ipc.inbox.remove(id);
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
            let request = proc.ipc.inbox.get_mut(id).unwrap();
            let data = request.kernel_data.as_mut().ok_or(Errno::EINVAL)?;
            let bytes = data.get_mut(..length).ok_or(Errno::EINVAL)?;
            copy_in_slice(&mut proc.pagetable, source, bytes).map_err(|_| Errno::EFAULT)?;
        } else if proc.pid == Some(message.sender) {
            let bytes = copy_in_cont::<u8>(&mut proc.pagetable, source, length)
                .map_err(|_| Errno::EFAULT)?;
            copy_out_cont(&mut proc.pagetable, destination, &bytes).map_err(|_| Errno::EFAULT)?;
        } else {
            let caller =
                find_process(&mut kernel.process_table, message.sender).ok_or(Errno::ESRCH)?;
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
        let request = proc.ipc.inbox.get_mut(id).unwrap();
        request.result = Some(result);
        request.state = RequestState::Completed;
        let destination = proc.pid.ok_or(Errno::ESRCH)?;
        for caller in &mut kernel.process_table {
            if caller.ipc.pending_call == Some(CurrentReq { destination, id }) {
                caller.wakeup_on(Some(channel(caller)));
            }
        }
        return Ok(());
    }

    let destination = proc.pid.ok_or(Errno::ESRCH)?;
    if message.sender == destination {
        proc.ipc
            .complete_outgoing(CurrentReq { destination, id }, result);
    } else if let Some(caller) = find_process(&mut kernel.process_table, message.sender) {
        if caller
            .ipc
            .complete_outgoing(CurrentReq { destination, id }, result)
        {
            caller.wakeup_on(Some(channel(caller)));
        }
    }
    proc.ipc.inbox.remove(id);
    Ok(())
}
