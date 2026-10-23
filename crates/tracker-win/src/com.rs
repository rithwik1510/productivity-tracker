//! Per-thread COM initialization.

use std::marker::PhantomData;

use windows::Win32::System::Com::{COINIT, CoInitializeEx, CoUninitialize};

/// COM initialized on the current thread for as long as this lives. Declare it after every COM
/// object it protects, so those are released first. `!Send`: it must drop on its own thread.
pub(crate) struct Apartment {
    initialized: bool,
    _not_send: PhantomData<*const ()>,
}

impl Apartment {
    pub(crate) fn enter(mode: COINIT) -> Apartment {
        // SAFETY: plain FFI call; balanced by CoUninitialize in Drop when it succeeded.
        let hr = unsafe { CoInitializeEx(None, mode) };
        if hr.is_err() {
            // RPC_E_CHANGED_MODE: the thread already has another apartment. COM still works,
            // just not in the mode we asked for, and we must not uninitialize it.
            tracing::warn!("CoInitializeEx failed: {hr:?}");
        }
        Apartment {
            initialized: hr.is_ok(),
            _not_send: PhantomData,
        }
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: balances the successful CoInitializeEx on this same thread (`!Send`).
            unsafe { CoUninitialize() };
        }
    }
}
