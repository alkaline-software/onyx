//! The keys pressed in Onyx's settings while a shortcut is being
//! recorded, read off AppKit's event before gpui sees it: its key code
//! and its flags, which are the numbers a hot key is held by, whatever
//! layout the person types in. While nothing is recorded every event
//! passes on as it came.

use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::channel::mpsc::{unbounded, UnboundedReceiver};
use objc2_app_kit::{NSEvent, NSEventMask};

static RECORDING: AtomicBool = AtomicBool::new(false);

/// The watch installed, once: each key pressed while `recording(true)`,
/// as its flags and key code. Such a key goes nowhere else.
pub fn install() -> UnboundedReceiver<(u64, u16)> {
    let (tx, rx) = unbounded();
    let block = block2::RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        if !RECORDING.load(Ordering::Relaxed) {
            return event.as_ptr();
        }
        let e = unsafe { event.as_ref() };
        let _ = tx.unbounded_send((e.modifierFlags().bits() as u64, e.keyCode()));
        std::ptr::null_mut()
    });
    let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block) };
    // Kept for the app's life.
    std::mem::forget(monitor);
    rx
}

pub fn recording(on: bool) {
    RECORDING.store(on, Ordering::Relaxed);
}
