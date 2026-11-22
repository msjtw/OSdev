use crate::tty::Tty;
use serverlib::{Result, tty::POLLIN};

#[derive(Default)]
pub struct ReadinessPublisher {
    last: Option<bool>,
}

impl ReadinessPublisher {
    /// Publish settled state before acknowledging a settings change.
    pub fn publish_then_ack(
        &mut self,
        tty: &Tty,
        id: Option<usize>,
        send: impl FnMut(bool) -> Result<()>,
        mut ack: impl FnMut(usize) -> Result<()>,
    ) -> Result<()> {
        self.publish(tty, send)?;
        if let Some(id) = id {
            ack(id)?;
        }
        Ok(())
    }

    /// Publish initial/changed levels for an assigned terminal, committing only successful updates.
    pub fn publish(&mut self, tty: &Tty, mut send: impl FnMut(bool) -> Result<()>) -> Result<()> {
        if tty.session_id.is_none() {
            return Ok(());
        }
        let readable = tty.poll(POLLIN) != 0;
        if self.last != Some(readable) {
            send(readable)?;
            self.last = Some(readable);
        }
        Ok(())
    }
}
