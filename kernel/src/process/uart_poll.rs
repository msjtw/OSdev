pub(crate) mod state;

use crate::{
    KERNEL,
    process::{ProcState, ipc},
};
use core::sync::atomic::{AtomicBool, Ordering};
use serverlib::{abi::Message, uart::POLL};

static PENDING: AtomicBool = AtomicBool::new(false);

/// Mark UART work from interrupt context without allocating, locking or doing IPC.
pub fn request_poll() {
    PENDING.store(true, Ordering::Relaxed);
}

/// Deliver coalesced polling requests and collect replies from scheduler context.
pub fn service() {
    let mut kernel = KERNEL.get().unwrap().lock();
    if PENDING.swap(false, Ordering::Relaxed) {
        kernel.uart_poll.tick();
    }
    let Some(pid) = kernel.uart_poll.destination() else {
        return;
    };
    let alive = kernel.process_table.iter().any(|proc| {
        proc.pid == Some(pid) && !matches!(proc.state, ProcState::Unused | ProcState::Zombie)
    });
    if !alive {
        if let Some(id) = kernel.uart_poll.disable() {
            if let Some(proc) = kernel
                .process_table
                .iter_mut()
                .find(|proc| proc.pid == Some(pid))
            {
                proc.ipc.inbox.remove(id);
            }
        }
        return;
    }
    if let Some(id) = kernel.uart_poll.outstanding() {
        if let Some((result, _)) = ipc::kernel_read_reply(&mut kernel, pid, id) {
            kernel.uart_poll.acknowledge(id);
            if result != 0 {
                kernel.uart_poll.disable();
                crate::log::error!("UART poll acknowledgement failed: {:#x}", result);
                return;
            }
        }
    }
    if let Some(destination) = kernel.uart_poll.ready() {
        let message = Message {
            operation: POLL,
            ..Message::default()
        };
        // Failure leaves state.pending set for another scheduler iteration.
        if let Ok(Some(id)) = ipc::kernel_send(&mut kernel, destination, message, None) {
            kernel.uart_poll.sent(id);
        }
    }
}
