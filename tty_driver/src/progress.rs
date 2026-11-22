use crate::{
    pending::PendingReads,
    tty::Tty,
    uart::{InputEffects, UartTransfers},
};
use serverlib::Result;

/// Retry retained input after reads free space, stopping only when local progress settles.
pub fn input(
    uart: &mut UartTransfers,
    tty: &mut Tty,
    reads: &mut PendingReads,
    mut reply: impl FnMut(usize, Result<usize>, Option<&[u8]>) -> Result<()>,
) -> Result<InputEffects> {
    let mut total = InputEffects::default();
    loop {
        let effects = uart.process_input(tty);
        total.interrupts += effects.interrupts;
        total.processed += effects.processed;
        total.blocked = effects.blocked;
        let completed = reads.complete(tty, &mut reply)?;
        if effects.processed == 0 && completed == 0 {
            return Ok(total);
        }
    }
}
