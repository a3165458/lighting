use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{bail, Result};
use windows::Win32::Foundation::{ERROR_SUCCESS, HANDLE};
use windows::Win32::System::Power::{
    PowerRegisterSuspendResumeNotification, PowerUnregisterSuspendResumeNotification,
    DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, HPOWERNOTIFY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DEVICE_NOTIFY_CALLBACK, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND, PBT_APMSUSPEND,
};

/// Stops a session on suspend or resume without requiring a window/message loop.
/// Display restoration remains the session owner's job, after the encoder stops.
pub struct SuspendGuard {
    registration: HPOWERNOTIFY,
    // Owns one Arc strong reference, released only after successful unregistration.
    context: *const AtomicBool,
}

// SAFETY: Registration/unregistration have no thread affinity. Moving the guard
// does not move the Arc allocation, and callbacks only access its AtomicBool.
// The raw pointer deliberately does not confer Sync on the guard.
unsafe impl Send for SuspendGuard {}

impl SuspendGuard {
    pub fn register(stop: Arc<AtomicBool>) -> Result<Self> {
        let context = Arc::into_raw(stop);
        let parameters = DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
            Callback: Some(on_power_event),
            Context: context.cast_mut().cast(),
        };
        let mut registration = std::ptr::null_mut();
        // SAFETY: The API reads the subscription parameters during registration.
        // Its callback context is an Arc allocation retained independently of this
        // stack frame, including when a callback arrives before registration returns.
        let status = unsafe {
            PowerRegisterSuspendResumeNotification(
                DEVICE_NOTIFY_CALLBACK,
                HANDLE(
                    (&parameters as *const DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS)
                        .cast_mut()
                        .cast(),
                ),
                &mut registration,
            )
        };
        if status != ERROR_SUCCESS {
            // SAFETY: Failed registration did not retain a subscription. Reclaim
            // exactly the strong reference transferred by Arc::into_raw above.
            unsafe { drop(Arc::from_raw(context)) };
            bail!(
                "PowerRegisterSuspendResumeNotification failed with Windows error {}",
                status.0
            );
        }
        Ok(Self {
            registration: HPOWERNOTIFY(registration as isize),
            context,
        })
    }
}

impl Drop for SuspendGuard {
    fn drop(&mut self) {
        // SAFETY: This guard exclusively owns the successful registration and
        // keeps the callback context alive until the subscription is cancelled.
        let status = unsafe { PowerUnregisterSuspendResumeNotification(self.registration) };
        if status == ERROR_SUCCESS {
            // SAFETY: The cancelled subscription can no longer use this context.
            // Reclaim exactly the one strong reference owned by the guard.
            unsafe { drop(Arc::from_raw(self.context)) };
        }
        // On failure the native subscription may still invoke the callback.
        // Intentionally leak our strong reference rather than leave Windows a
        // dangling context pointer. It only retains the old session's stop flag.
    }
}

unsafe extern "system" fn on_power_event(
    context: *const c_void,
    event: u32,
    _setting: *const c_void,
) -> u32 {
    if !context.is_null()
        && matches!(
            event,
            PBT_APMSUSPEND | PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND
        )
    {
        // SAFETY: Registration supplies the retained Arc allocation. No mutable
        // reference, allocation, lock, display work, or fallible operation occurs
        // on this native callback thread; the stop signal is never cleared here.
        unsafe { &*context.cast::<AtomicBool>() }.store(true, Ordering::Release);
    }
    ERROR_SUCCESS.0
}
