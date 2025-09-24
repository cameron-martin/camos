use core::{
    cell::UnsafeCell,
    hint,
    ops::{Deref, DerefMut},
    sync::atomic::{self, AtomicBool},
};

pub struct SpinLock<T: ?Sized> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized + Send> Send for SpinLock<T> {}
unsafe impl<T: ?Sized + Send> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> SpinLockGuard<T> {
        loop {
            let lock_result = self.locked.compare_exchange(
                false,
                true,
                atomic::Ordering::Acquire,
                atomic::Ordering::Relaxed,
            );

            if lock_result.is_ok() {
                return SpinLockGuard { lock: &self };
            }

            hint::spin_loop();
        }
    }
}

pub struct SpinLockGuard<'a, T> {
    lock: &'a SpinLock<T>,
}

impl<'a, T> Drop for SpinLockGuard<'a, T> {
    fn drop(&mut self) {
        if let Err(_) = self.lock.locked.compare_exchange(
            true,
            false,
            atomic::Ordering::Release,
            atomic::Ordering::Relaxed,
        ) {
            panic!("attempted to unlock and already-unlocked spinlock");
        }
    }
}

impl<'a, T> Deref for SpinLockGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: Having a reference to the lock guard means we have exclusive
        // access to the data inside the lock.
        unsafe { &*self.lock.data.get() }
    }
}

impl<'a, T> DerefMut for SpinLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: Having a reference to the lock guard means we have exclusive
        // access to the data inside the lock.
        unsafe { &mut *self.lock.data.get() }
    }
}

#[cfg(test)]
mod tests {
    use crate::spinlock::SpinLock;

    /// Tests the interface of the spinlock, but nothing about the concurrency
    /// properties of it
    #[test]
    fn spinlock_usage() {
        static COUNTER: SpinLock<u64> = SpinLock::new(0);

        for _ in 0..100 {
            let mut counter = COUNTER.lock();

            *counter += 1;
        }

        assert_eq!(*COUNTER.lock(), 100);
    }
}
