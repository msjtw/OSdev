#[derive(Debug, Default)]
pub struct PollState {
    destination: Option<usize>,
    outstanding: Option<usize>,
    pending: bool,
}

impl PollState {
    /// Register the boot UART PID before notification delivery begins.
    pub fn configure(&mut self, pid: usize) {
        assert!(self.outstanding.is_none());
        self.destination = Some(pid);
        self.pending = false;
    }

    /// Coalesce one or more elapsed ticks into pending work.
    pub fn tick(&mut self) {
        self.pending = true;
    }

    /// Return the configured destination for liveness checking.
    pub fn destination(&self) -> Option<usize> {
        self.destination
    }

    /// Return the request ID whose acknowledgement must be collected.
    pub fn outstanding(&self) -> Option<usize> {
        self.outstanding
    }

    /// Select the destination only when work is pending and no request is outstanding.
    pub fn ready(&self) -> Option<usize> {
        if self.pending && self.outstanding.is_none() {
            self.destination
        } else {
            None
        }
    }

    /// Commit a successful enqueue without discarding ticks arriving afterward.
    pub fn sent(&mut self, id: usize) {
        assert!(self.ready().is_some());
        self.outstanding = Some(id);
        self.pending = false;
    }

    /// Release the matching outstanding request after its acknowledgement is collected.
    pub fn acknowledge(&mut self, id: usize) {
        assert_eq!(self.outstanding, Some(id));
        self.outstanding = None;
    }

    /// Disable a dead or invalid peer and return any request needing removal.
    pub fn disable(&mut self) -> Option<usize> {
        self.destination = None;
        self.pending = false;
        self.outstanding.take()
    }
}
