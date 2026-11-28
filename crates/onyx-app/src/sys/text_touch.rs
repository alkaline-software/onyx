//! What a finger has in words on an iPhone and an iPad, as the system's
//! own text views have it: the edit menu over a selection or a caret
//! (`UIEditMenuInteraction`, the system's Cut, Copy and Paste bar) and the
//! loupe that rises over the finger while it moves the caret or an end of
//! the selection (`UITextLoupeSession`). gpui reads the finger and the
//! editor places the selection; these are the system's pieces it shows.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;

use gpui::{Bounds, Pixels, Point};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSArray, NSString};
use objc2_ui_kit::{UIAction, UIEditMenuConfiguration, UIEditMenuInteraction, UIEditMenuInteractionAnimating, UIEditMenuInteractionDelegate, UIMenu, UIMenuElement, UITextLoupeSession, UIView};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::EditChoice;

thread_local! {
    static MENU: RefCell<Option<(Retained<UIEditMenuInteraction>, Retained<MenuDelegate>, usize)>> = const { RefCell::new(None) };
    /// What the menu stands over, in the window's points, and what it offers.
    static AT: Cell<CGRect> = const { Cell::new(CGRect { origin: CGPoint { x: 0., y: 0. }, size: CGSize { width: 0., height: 0. } }) };
    static CHOICES: RefCell<Vec<EditChoice>> = const { RefCell::new(Vec::new()) };
    static SHOWN: Cell<bool> = const { Cell::new(false) };
    static LOUPE: RefCell<Option<Retained<UITextLoupeSession>>> = const { RefCell::new(None) };
    static SAYS: RefCell<Option<futures::channel::mpsc::UnboundedSender<EditChoice>>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "OnyxEditMenuDelegate"]
    struct MenuDelegate;

    unsafe impl NSObjectProtocol for MenuDelegate {}

    unsafe impl UIEditMenuInteractionDelegate for MenuDelegate {
        #[unsafe(method_id(editMenuInteraction:menuForConfiguration:suggestedActions:))]
        fn menu(&self, _interaction: &UIEditMenuInteraction, _configuration: &UIEditMenuConfiguration, _suggested: &NSArray<UIMenuElement>) -> Option<Retained<UIMenu>> {
            let mtm = self.mtm();
            let items: Vec<Retained<UIMenuElement>> = CHOICES.with(|c| c.borrow().clone()).into_iter().map(|choice| {
                let handler = block2::RcBlock::new(move |_: NonNull<UIAction>| {
                    SAYS.with(|s| {
                        if let Some(tx) = s.borrow().as_ref() {
                            let _ = tx.unbounded_send(choice);
                        }
                    });
                });
                let title = NSString::from_str(&crate::i18n::t(choice.word()));
                // SAFETY: a block UIKit copies, alive through the call, on the main thread.
                let action = unsafe { UIAction::actionWithTitle_image_identifier_handler(&title, None, None, &*handler as *const _ as *mut _, mtm) };
                Retained::into_super(action)
            }).collect();
            Some(UIMenu::menuWithChildren(&NSArray::from_retained_slice(&items), mtm))
        }

        #[unsafe(method(editMenuInteraction:targetRectForConfiguration:))]
        fn target(&self, _interaction: &UIEditMenuInteraction, _configuration: &UIEditMenuConfiguration) -> CGRect {
            AT.with(Cell::get)
        }

        #[unsafe(method(editMenuInteraction:willPresentMenuForConfiguration:animator:))]
        fn presents(&self, _interaction: &UIEditMenuInteraction, _configuration: &UIEditMenuConfiguration, _animator: &ProtocolObject<dyn UIEditMenuInteractionAnimating>) {
            SHOWN.with(|s| s.set(true));
        }

        #[unsafe(method(editMenuInteraction:willDismissMenuForConfiguration:animator:))]
        fn dismisses(&self, _interaction: &UIEditMenuInteraction, _configuration: &UIEditMenuConfiguration, _animator: &ProtocolObject<dyn UIEditMenuInteractionAnimating>) {
            SHOWN.with(|s| s.set(false));
        }
    }
);

/// Where the menu's choices go: one listener, the window's.
pub fn listen() -> futures::channel::mpsc::UnboundedReceiver<EditChoice> {
    let (tx, rx) = futures::channel::mpsc::unbounded();
    SAYS.with(|s| *s.borrow_mut() = Some(tx));
    rx
}

fn host(window: &gpui::Window) -> Option<&UIView> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::UiKit(handle) = handle.as_raw() else { return None };
    // SAFETY: the handle's view is the window's own UIView, alive while the window is.
    Some(unsafe { handle.ui_view.cast::<UIView>().as_ref() })
}

fn rect(b: Bounds<Pixels>) -> CGRect {
    CGRect::new(CGPoint::new(f64::from(f32::from(b.origin.x)), f64::from(f32::from(b.origin.y))), CGSize::new(f64::from(f32::from(b.size.width)), f64::from(f32::from(b.size.height))))
}

/// The interaction on the window's view, made once for it.
fn interaction(window: &gpui::Window, mtm: MainThreadMarker) -> Option<Retained<UIEditMenuInteraction>> {
    let view = host(window)?;
    let key = view as *const UIView as usize;
    MENU.with(|m| {
        let mut m = m.borrow_mut();
        if m.as_ref().is_none_or(|(_, _, k)| *k != key) {
            let delegate: Retained<MenuDelegate> = unsafe { msg_send![MenuDelegate::alloc(mtm), init] };
            let made = UIEditMenuInteraction::initWithDelegate(UIEditMenuInteraction::alloc(mtm), Some(ProtocolObject::from_ref(&*delegate)));
            // SAFETY: the interaction added to the window's own view, on the main thread.
            let _: () = unsafe { msg_send![view, addInteraction: &*made] };
            *m = Some((made, delegate, key));
        }
        m.as_ref().map(|(i, _, _)| i.clone())
    })
}

pub fn show(window: &gpui::Window, at: Bounds<Pixels>, choices: Vec<EditChoice>) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some(menu) = interaction(window, mtm) else { return };
    AT.with(|a| a.set(rect(at)));
    CHOICES.with(|c| *c.borrow_mut() = choices);
    let r = rect(at);
    let point = CGPoint::new(r.origin.x + r.size.width / 2., r.origin.y);
    // SAFETY: a configuration with no identifier, on the main thread.
    let configuration = unsafe { UIEditMenuConfiguration::configurationWithIdentifier_sourcePoint(None, point, mtm) };
    // Over what it stands for, as the system's text views have it, where there is room (UIEditMenuArrowDirectionDown, 2).
    // SAFETY: an NSInteger property of the configuration, on the main thread.
    let _: () = unsafe { msg_send![&*configuration, setPreferredArrowDirection: 2isize] };
    menu.presentEditMenuWithConfiguration(&configuration);
}

pub fn follow(at: Bounds<Pixels>) {
    if !shown() {
        return;
    }
    let r = rect(at);
    if AT.with(Cell::get) == r {
        return;
    }
    AT.with(|a| a.set(r));
    MENU.with(|m| {
        if let Some((menu, _, _)) = m.borrow().as_ref() {
            menu.updateVisibleMenuPositionAnimated(false);
        }
    });
}

pub fn hide() {
    if !shown() {
        return;
    }
    MENU.with(|m| {
        if let Some((menu, _, _)) = m.borrow().as_ref() {
            menu.dismissMenu();
        }
    });
    SHOWN.with(|s| s.set(false));
}

pub fn shown() -> bool {
    SHOWN.with(Cell::get)
}

pub fn loupe(window: &gpui::Window, at: Option<(Point<Pixels>, Bounds<Pixels>)>) {
    LOUPE.with(|l| {
        let mut l = l.borrow_mut();
        let Some((finger, caret)) = at else {
            if let Some(session) = l.take() {
                session.invalidate();
            }
            return;
        };
        let point = CGPoint::new(f64::from(f32::from(finger.x)), f64::from(f32::from(finger.y)));
        if l.is_none() {
            let Some(view) = host(window) else { return };
            *l = UITextLoupeSession::beginLoupeSessionAtPoint_fromSelectionWidgetView_inView(point, None, view);
        }
        if let Some(session) = l.as_ref() {
            session.moveToPoint_withCaretRect_trackingCaret(point, rect(caret), false);
        }
    });
}
