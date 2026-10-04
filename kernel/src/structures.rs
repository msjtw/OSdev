/// Fixed-capacity byte FIFO with no heap allocation.
///
/// `CAPACITY` must be nonzero.
#[derive(Debug)]
pub struct RingBuffer<const CAPACITY: usize> {
    bytes: [u8; CAPACITY],
    head: usize,
    len: usize,
}

impl<const CAPACITY: usize> RingBuffer<CAPACITY> {
    pub const fn new() -> Self {
        Self {
            bytes: [0; CAPACITY],
            head: 0,
            len: 0,
        }
    }

    pub fn push(&mut self, byte: u8) -> bool {
        if self.len == CAPACITY {
            return false;
        }

        let tail = (self.head + self.len) % CAPACITY;
        self.bytes[tail] = byte;
        self.len += 1;
        true
    }

    pub fn pop(&mut self) -> Option<u8> {
        if self.len == 0 {
            return None;
        }

        let byte = self.bytes[self.head];
        self.head = (self.head + 1) % CAPACITY;
        self.len -= 1;
        Some(byte)
    }

    pub const fn available(&self) -> usize {
        self.len
    }

    pub const fn free_space(&self) -> usize {
        CAPACITY - self.len
    }
}

impl<const CAPACITY: usize> Default for RingBuffer<CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}
