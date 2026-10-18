use core::ops::{Deref, DerefMut};
use spin::{Mutex, MutexGuard};

#[derive(Default, Debug)]
pub struct IntMutex<T>(Mutex<T>);

pub struct IntMutexGuard<'a, T>(Option<MutexGuard<'a, T>>);

impl<T> IntMutex<T> {
    pub const fn new(value: T) -> Self {
        Self(Mutex::new(value))
    }

    pub fn lock(&self) -> IntMutexGuard<'_, T> {
        unsafe { crate::CPU.push_interrupt_off() };

        let guard = self.0.lock();

        // uart_print("MUTEX LOCKED\n");

        IntMutexGuard(Some(guard))
    }

    pub fn try_lock(&self) -> Option<IntMutexGuard<'_, T>> {
        unsafe { crate::CPU.push_interrupt_off() };
        match self.0.try_lock() {
            Some(guard) => Some(IntMutexGuard(Some(guard))),
            None => {
                unsafe { crate::CPU.pop_interrupt_off() };
                None
            }
        }
    }

    pub unsafe fn lock_manual(&self) {
        unsafe {
            crate::CPU.push_interrupt_off();

            let guard = self.0.lock();

            // Do NOT let MutexGuard::drop() unlock the mutex.
            core::mem::forget(guard);
        }
    }

    pub unsafe fn unlock_manual(&self) {
        unsafe {
            self.0.force_unlock();

            crate::CPU.pop_interrupt_off();
        }
    }
}

impl<T> Deref for IntMutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.0.as_ref().unwrap().deref()
    }
}

impl<T> DerefMut for IntMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.0.as_mut().unwrap().deref_mut()
    }
}

impl<T> Drop for IntMutexGuard<'_, T> {
    fn drop(&mut self) {
        self.0.take();

        unsafe { crate::CPU.pop_interrupt_off() };

        // uart_print("MUTEX UNLOCKED\n");
    }
}
