//! The shortcuts, as the system's own hot keys (Carbon's
//! `RegisterEventHotKey`, which every Mac app that answers a key from
//! anywhere has used). A hot key needs no permission and no event tap:
//! the system hands Onyx the press and nothing else, so no other key is
//! seen and nothing waits on Onyx while the person types.
//!
//! The press arrives on the main thread, through the app's event target,
//! and goes on down a channel as the shortcut's number. Taken from Mado
//! (`mado/crates/mado-app/src/sys/hotkeys.rs`), where every window action
//! has one; Onyx holds one, the menu bar note's (`menubar.rs`).

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::OnceLock;

use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use onyx_core::shortcut::{Shortcut, ALT, CMD, CTRL, SHIFT};

type Ref = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct HotKeyId {
    signature: u32,
    id: u32,
}

#[repr(C)]
struct EventType {
    class: u32,
    kind: u32,
}

type Handler = extern "C" fn(next: Ref, event: Ref, user: Ref) -> i32;

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn GetApplicationEventTarget() -> Ref;
    fn InstallEventHandler(target: Ref, handler: Handler, count: u32, types: *const EventType, user: Ref, out: *mut Ref) -> i32;
    fn RegisterEventHotKey(code: u32, mods: u32, id: HotKeyId, target: Ref, options: u32, out: *mut Ref) -> i32;
    fn UnregisterEventHotKey(hot_key: Ref) -> i32;
    fn GetEventParameter(event: Ref, name: u32, kind: u32, actual_kind: *mut u32, size: usize, actual_size: *mut usize, data: *mut c_void) -> i32;
}

const fn four(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

const SIGNATURE: u32 = four(b"onyx");
const CLASS_KEYBOARD: u32 = four(b"keyb");
const HOT_KEY_PRESSED: u32 = 5;
const PARAM_DIRECT: u32 = four(b"----");
const TYPE_HOT_KEY_ID: u32 = four(b"hkid");

/// Carbon's modifier bits, which are not the event flags the rest of the
/// app keeps.
fn carbon_mods(mods: u64) -> u32 {
    let mut out = 0;
    if mods & CMD != 0 {
        out |= 1 << 8;
    }
    if mods & SHIFT != 0 {
        out |= 1 << 9;
    }
    if mods & ALT != 0 {
        out |= 1 << 11;
    }
    if mods & CTRL != 0 {
        out |= 1 << 12;
    }
    out
}

static PRESSES: OnceLock<UnboundedSender<u32>> = OnceLock::new();

thread_local! {
    /// The hot keys held now, to give back before a new set is taken.
    static HELD: RefCell<Vec<Ref>> = const { RefCell::new(Vec::new()) };
}

extern "C" fn pressed(_next: Ref, event: Ref, _user: Ref) -> i32 {
    let mut id = HotKeyId::default();
    let err = unsafe { GetEventParameter(event, PARAM_DIRECT, TYPE_HOT_KEY_ID, std::ptr::null_mut(), std::mem::size_of::<HotKeyId>(), std::ptr::null_mut(), &mut id as *mut HotKeyId as *mut c_void) };
    if err == 0 && id.signature == SIGNATURE {
        if let Some(tx) = PRESSES.get() {
            let _ = tx.unbounded_send(id.id);
        }
    }
    0
}

/// The handler installed, once, and the presses it hears: each the number
/// a shortcut was given in `set`.
pub fn install() -> Option<UnboundedReceiver<u32>> {
    let (tx, rx) = unbounded();
    PRESSES.set(tx).ok()?;
    let types = [EventType { class: CLASS_KEYBOARD, kind: HOT_KEY_PRESSED }];
    let err = unsafe { InstallEventHandler(GetApplicationEventTarget(), pressed, 1, types.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut()) };
    (err == 0).then_some(rx)
}

/// The shortcuts held from now on, each with its number; the old ones are
/// given back first. The numbers of those the system would not take are
/// returned.
pub fn set(keys: &[(u32, Shortcut)]) -> Vec<u32> {
    HELD.with(|held| {
        let mut held = held.borrow_mut();
        for r in held.drain(..) {
            unsafe { UnregisterEventHotKey(r) };
        }
        let mut refused = Vec::new();
        for &(n, s) in keys {
            let mut out: Ref = std::ptr::null_mut();
            let err = unsafe { RegisterEventHotKey(s.key as u32, carbon_mods(s.mods), HotKeyId { signature: SIGNATURE, id: n }, GetApplicationEventTarget(), 0, &mut out) };
            if err == 0 && !out.is_null() {
                held.push(out);
            } else {
                refused.push(n);
            }
        }
        refused
    })
}
