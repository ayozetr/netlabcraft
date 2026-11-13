//! A mutex that never poisons.
//!
//! `std::sync::Mutex` turns one panic while locked into a panic on every later `lock()`. NetLabCraft
//! catches escaped panics at its edge (`Session::execute`) and carries on, so a poisoned lock must not
//! take the rest of the program down with it. This wrapper hands the data back anyway: the guarded
//! values here are plain state that stays valid after an interrupted update.
//!
//! Use this type instead of `std::sync::Mutex` (clippy refuses the std one, see `clippy.toml`).
#![allow(clippy::disallowed_types)]

use std::sync::{Mutex as StdMutex, MutexGuard};

/// A mutex whose `lock()` cannot fail.
#[derive(Debug, Default)]
pub struct Mutex<T>(StdMutex<T>);

impl<T> Mutex<T> {
    pub fn new(value: T) -> Self {
        Mutex(StdMutex::new(value))
    }

    /// Locks, recovering the data if an earlier holder panicked.
    pub fn lock(&self) -> MutexGuard<'_, T> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn lock_survives_a_panic_in_another_holder() {
        let m = Arc::new(Mutex::new(1));
        let m2 = Arc::clone(&m);
        let _ = std::thread::spawn(move || {
            let mut g = m2.lock();
            *g = 2;
            panic!("holder panics while locked");
        })
        .join();
        assert_eq!(*m.lock(), 2);
    }
}
