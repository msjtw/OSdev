use core::ops::{Deref, DerefMut};
use spin::{Mutex, MutexGuard};

// Wrapper around spin::mutex that disables interrupts
#[derive(Default)]
pub struct IntMutex<T>(Mutex<T>);

pub struct IntMutexGuard<'a, T>(MutexGuard<'a, T>);

impl<T> IntMutex<T> {
    pub const fn new(value: T) -> Self {
        Self(Mutex::new(value))
    }

    pub fn lock(&self) -> IntMutexGuard<'_, T> {
        unsafe { crate::CPU.push_interrupt_off() };
        IntMutexGuard(self.0.lock())
    }
}

impl<T> Deref for IntMutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for IntMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T> Drop for IntMutexGuard<'_, T> {
    fn drop(&mut self) {
        unsafe { crate::CPU.pop_interrupt_off() };
    }
}
