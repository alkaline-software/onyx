//! Onyx's item in the menu bar: the way to the menu bar note
//! (`menubar.rs`), as MiniNotes was, and a menu of the app's own.
//!
//! The item is the app icon in one ink: the page with its three lines,
//! the pen writing on the middle one, on the slab of onyx with its vein
//! cut across it (`icons/menubar-note.svg`); while the note is open the
//! page is filled and its lines cut out of it (`menubar-note-open.svg`), as
//! the system's input menu badge fills while it is engaged. Where two parts
//! cross, the one behind is cut back by a gap, since a template image has
//! one ink and an overlap would merge them. Both are template images the
//! system tints, `ITEM_SIZE`, drawn at one and two pixels a point.
//!
//! A left click opens or closes the note; a right click, or Control and a
//! click, shows the menu. The item has no menu of its own: the button
//! sends both clicks to an Objective-C target, which reads the click from
//! the app's current event and either sends `Pick::Note` down the channel
//! or builds the menu from the lines the app last handed over
//! (`set_lines`), sets it, clicks the button and takes it off again once
//! it closes. An item with a menu set would show it for either click.
//! Taken from Mado (`mado/crates/mado-app/src/sys/status_item.rs`).
//!
//! A menu bar manager (Ice, Hidden Bar) hides an item by pushing it off
//! the screen, and the system keeps that place in the app's own defaults,
//! so the item could stay out of sight across launches. `heal` puts it
//! back at a place in sight, as MiniNotes did: at launch, once the bar has
//! settled, before the note opens, and when Onyx is opened again.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol as _};
use objc2::{define_class, msg_send, sel, AllocAnyThread as _, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSBitmapImageRep, NSEventMask, NSEventModifierFlags, NSEventType, NSImage, NSMenu, NSMenuItem, NSScreen, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::{NSData, NSObject, NSSize, NSString, NSUserDefaults};

use super::{ItemLine, ItemPick};

static PICKS: OnceLock<UnboundedSender<ItemPick>> = OnceLock::new();

/// The name the system keeps the item's place under, and the default it
/// keeps it in.
const AUTOSAVE: &str = "OnyxNote";
/// From the bar's right end: the rightmost place an app's item may have,
/// always in sight.
const HEALED_PLACE: f64 = 50.;

/// The item's picture, in points.
const ITEM_SIZE: NSSize = NSSize::new(18., 16.);
const CLOSED: &[u8] = include_bytes!("../../assets/icons/menubar-note.svg");
const OPEN: &[u8] = include_bytes!("../../assets/icons/menubar-note-open.svg");

thread_local! {
    /// The item, kept for the app's life, and whether the note is open.
    static ITEM: RefCell<Option<Retained<NSStatusItem>>> = const { RefCell::new(None) };
    static OPEN_NOW: RefCell<bool> = const { RefCell::new(false) };
    static SHOWN: RefCell<bool> = const { RefCell::new(true) };
    /// What the item and the menu call, which they hold weakly.
    static TARGET: RefCell<Option<Retained<MenuTarget>>> = const { RefCell::new(None) };
    /// The menu's lines as the app last handed them, and each item's key
    /// by its tag once built.
    static LINES: RefCell<Vec<ItemLine>> = const { RefCell::new(Vec::new()) };
    static KEYS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Each picture drawn once, by where its bytes are.
    static IMAGES: RefCell<HashMap<usize, Retained<NSImage>>> = RefCell::new(HashMap::new());
}

fn send(pick: ItemPick) {
    if let Some(tx) = PICKS.get() {
        let _ = tx.unbounded_send(pick);
    }
}

define_class!(
    // The item and the menu's choices call here, on the main thread, and
    // what was asked goes on to the app as a message.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "OnyxMenuTarget"]
    struct MenuTarget;

    impl MenuTarget {
        /// A click on the item, either button. A left click with Control
        /// held is a right one, as everywhere on a Mac.
        #[unsafe(method(clicked:))]
        fn clicked(&self, _sender: Option<&AnyObject>) {
            let mtm = self.mtm();
            let right = NSApplication::sharedApplication(mtm).currentEvent().is_some_and(|e| e.r#type() == NSEventType::RightMouseUp || e.modifierFlags().contains(NSEventModifierFlags::Control));
            if right {
                send(ItemPick::Menu);
                show_menu(mtm, self);
            } else {
                send(ItemPick::Note);
            }
        }

        #[unsafe(method(pick:))]
        fn pick(&self, sender: Option<&NSMenuItem>) {
            let key = sender.and_then(|s| KEYS.with(|k| k.borrow().get(usize::try_from(s.tag()).ok()?).cloned()));
            if let Some(key) = key {
                send(ItemPick::Line(key));
            }
        }
    }
);

/// The menu, built from the lines last handed over and set on the item
/// while it is shown, so the click that opens the note reaches the
/// target; the click below returns once the menu has closed.
fn show_menu(mtm: MainThreadMarker, target: &MenuTarget) {
    let Some(status) = ITEM.with(|i| i.borrow().clone()) else { return };
    let menu = KEYS.with(|keys| {
        let mut keys = keys.borrow_mut();
        keys.clear();
        LINES.with(|lines| build(mtm, target, &lines.borrow(), &mut keys))
    });
    status.setMenu(Some(&menu));
    if let Some(button) = status.button(mtm) {
        unsafe { button.performClick(None) };
    }
    status.setMenu(None);
}

fn build(mtm: MainThreadMarker, target: &MenuTarget, lines: &[ItemLine], keys: &mut Vec<String>) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    // The items' enabled state is the app's, not AppKit's guess.
    menu.setAutoenablesItems(false);
    for line in lines {
        match line {
            ItemLine::Gap => menu.addItem(&NSMenuItem::separatorItem(mtm)),
            ItemLine::Item { key, title, icon, keys: (equivalent, mask) } => {
                let it = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str(title), Some(sel!(pick:)), &NSString::from_str(equivalent)) };
                it.setKeyEquivalentModifierMask(NSEventModifierFlags::from_bits_retain(*mask as _));
                unsafe { it.setTarget(Some(target)) };
                it.setTag(keys.len() as isize);
                keys.push(key.clone());
                it.setEnabled(true);
                picture(&it, icon);
                menu.addItem(&it);
            }
        }
    }
    menu
}

fn picture(item: &NSMenuItem, svg: &'static [u8]) {
    if svg.is_empty() {
        return;
    }
    let image = cached(svg, NSSize::new(16., 16.));
    item.setImage(Some(&image));
    // An app built on the macOS 27 SDK has its menus' images hidden unless
    // each item asks for its own (`.visible` = 1).
    if item.respondsToSelector(sel!(setPreferredImageVisibility:)) {
        let _: () = unsafe { msg_send![item, setPreferredImageVisibility: 1isize] };
    }
}

fn cached(svg: &'static [u8], size: NSSize) -> Retained<NSImage> {
    IMAGES.with(|cache| cache.borrow_mut().entry(svg.as_ptr() as usize).or_insert_with(|| template(svg, size).unwrap_or_else(|| NSImage::initWithSize(NSImage::alloc(), size))).clone())
}

/// A template image of `size` points from an SVG, drawn for one and two
/// pixels a point, which the system tints.
fn template(svg: &[u8], size: NSSize) -> Option<Retained<NSImage>> {
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default()).ok()?;
    let image = NSImage::initWithSize(NSImage::alloc(), size);
    for scale in [1u32, 2] {
        let (w, h) = ((size.width as u32) * scale, (size.height as u32) * scale);
        let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;
        let k = (w as f32 / tree.size().width()).min(h as f32 / tree.size().height());
        resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(k, k), &mut pixmap.as_mut());
        let png = pixmap.encode_png().ok()?;
        let rep = NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(&png))?;
        rep.setSize(size);
        image.addRepresentation(&rep);
    }
    image.setTemplate(true);
    Some(image)
}

/// A new item, its badge and its clicks set, under the name its place is
/// kept by.
fn make(mtm: MainThreadMarker, target: &MenuTarget) -> Retained<NSStatusItem> {
    let status = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    status.setAutosaveName(Some(&NSString::from_str(AUTOSAVE)));
    if let Some(button) = status.button(mtm) {
        let open = OPEN_NOW.with(|o| *o.borrow());
        button.setImage(Some(&cached(if open { OPEN } else { CLOSED }, ITEM_SIZE)));
        button.setToolTip(Some(&NSString::from_str("Onyx")));
        unsafe {
            button.setTarget(Some(target));
            button.setAction(Some(sel!(clicked:)));
        }
        button.sendActionOn(NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp);
    }
    status.setVisible(SHOWN.with(|s| *s.borrow()));
    status
}

/// The item in the menu bar, shown or not, and what is chosen from it.
pub fn install(shown: bool) -> Option<UnboundedReceiver<ItemPick>> {
    let mtm = MainThreadMarker::new()?;
    let (tx, rx) = unbounded();
    PICKS.set(tx).ok()?;
    SHOWN.with(|s| *s.borrow_mut() = shown);
    let target: Retained<MenuTarget> = unsafe { msg_send![MenuTarget::alloc(mtm), init] };
    let status = make(mtm, &target);
    ITEM.with(|i| *i.borrow_mut() = Some(status));
    TARGET.with(|t| *t.borrow_mut() = Some(target));
    Some(rx)
}

/// Whether the item is in the bar at all.
pub fn set_shown(shown: bool) {
    SHOWN.with(|s| *s.borrow_mut() = shown);
    ITEM.with(|i| {
        if let Some(status) = i.borrow().as_ref() {
            status.setVisible(shown);
        }
    });
}

/// The badge filled while the note is open, outlined while it is not.
pub fn set_open(open: bool) {
    OPEN_NOW.with(|o| *o.borrow_mut() = open);
    let Some(mtm) = MainThreadMarker::new() else { return };
    ITEM.with(|i| {
        if let Some(button) = i.borrow().as_ref().and_then(|s| s.button(mtm)) {
            button.setImage(Some(&cached(if open { OPEN } else { CLOSED }, ITEM_SIZE)));
        }
    });
}

/// The menu's lines from now on: built when the menu is next shown.
pub fn set_lines(lines: Vec<ItemLine>) {
    LINES.with(|l| *l.borrow_mut() = lines);
}

/// Where the item stands, in gpui's terms: from the top left of the
/// screen the menu bar is on (the screens' origin is the first screen's
/// bottom left in AppKit's terms), as x, y, width and height. None while
/// it is hidden or off every screen.
pub fn frame() -> Option<(f32, f32, f32, f32)> {
    let mtm = MainThreadMarker::new()?;
    let status = ITEM.with(|i| i.borrow().clone())?;
    if !status.isVisible() {
        return None;
    }
    let window = status.button(mtm)?.window()?;
    let f = window.frame();
    let screens = NSScreen::screens(mtm);
    let first = screens.iter().next()?.frame();
    let on = screens.iter().any(|s| {
        let s = s.frame();
        f.origin.x + f.size.width > s.origin.x && f.origin.x < s.origin.x + s.size.width && f.origin.y + f.size.height > s.origin.y && f.origin.y < s.origin.y + s.size.height
    });
    if !on {
        return None;
    }
    let top = first.size.height - (f.origin.y + f.size.height);
    Some((f.origin.x as f32, top as f32, f.size.width as f32, f.size.height as f32))
}

/// The item made again at a place in sight when it stands off every
/// screen. Writing the place's default before making it is what puts the
/// new one there.
pub fn heal() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    if !SHOWN.with(|s| *s.borrow()) || frame().is_some() {
        return;
    }
    let Some(target) = TARGET.with(|t| t.borrow().clone()) else { return };
    if let Some(old) = ITEM.with(|i| i.borrow_mut().take()) {
        NSStatusBar::systemStatusBar().removeStatusItem(&old);
    }
    let key = NSString::from_str(&format!("NSStatusItem Preferred Position {AUTOSAVE}"));
    NSUserDefaults::standardUserDefaults().setDouble_forKey(HEALED_PLACE, &key);
    let status = make(mtm, &target);
    ITEM.with(|i| *i.borrow_mut() = Some(status));
}
