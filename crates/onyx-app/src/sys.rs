//! What is asked of the system, one function a job with a body for each
//! system: nothing else in the app shells out to `open`, `osascript` or
//! `id`, and nothing else names a system's own API.

use crate::i18n::phrase;

#[cfg(target_os = "macos")]
mod hotkeys;
#[cfg(target_os = "macos")]
mod keys;
#[cfg(target_os = "macos")]
mod status_item;
#[cfg(target_os = "ios")]
mod text_touch;

/// A choice of the edit menu a finger's selection brings (`edit_menu`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditChoice {
    Cut,
    Copy,
    Paste,
    Select,
    SelectAll,
}

impl EditChoice {
    #[cfg_attr(not(target_os = "ios"), allow(dead_code))]
    pub fn word(self) -> &'static str {
        match self {
            EditChoice::Cut => phrase("Cut"),
            EditChoice::Copy => phrase("Copy"),
            EditChoice::Paste => phrase("Paste"),
            EditChoice::Select => phrase("Select"),
            EditChoice::SelectAll => phrase("Select All"),
        }
    }
}

/// The system's edit menu over a selection or a caret on an iPhone and an
/// iPad, as its own text views have it (`sys/text_touch.rs`): shown over
/// `at` with these choices, which come back through `listen_edit_menu`.
/// Nowhere else, where a right click has the app's own menu.
#[cfg(target_os = "ios")]
pub fn edit_menu(window: &gpui::Window, at: gpui::Bounds<gpui::Pixels>, choices: Vec<EditChoice>) {
    text_touch::show(window, at, choices);
}
#[cfg(not(target_os = "ios"))]
pub fn edit_menu(_: &gpui::Window, _: gpui::Bounds<gpui::Pixels>, _: Vec<EditChoice>) {}

/// The edit menu, while it shows, stands over `at` now (a scroll moved
/// what it was over).
#[cfg(target_os = "ios")]
pub fn edit_menu_follow(at: gpui::Bounds<gpui::Pixels>) {
    text_touch::follow(at);
}
#[cfg(not(target_os = "ios"))]
pub fn edit_menu_follow(_: gpui::Bounds<gpui::Pixels>) {}

/// The edit menu put away.
#[cfg(target_os = "ios")]
pub fn edit_menu_hide() {
    text_touch::hide();
}
#[cfg(not(target_os = "ios"))]
pub fn edit_menu_hide() {}

/// Whether the edit menu shows.
#[cfg(target_os = "ios")]
pub fn edit_menu_shown() -> bool {
    text_touch::shown()
}
#[cfg(not(target_os = "ios"))]
pub fn edit_menu_shown() -> bool {
    false
}

/// Where the edit menu's choices go: one listener, the main window's.
#[cfg(target_os = "ios")]
pub fn listen_edit_menu() -> Option<futures::channel::mpsc::UnboundedReceiver<EditChoice>> {
    Some(text_touch::listen())
}
#[cfg(not(target_os = "ios"))]
pub fn listen_edit_menu() -> Option<futures::channel::mpsc::UnboundedReceiver<EditChoice>> {
    None
}

/// The system's loupe over a finger moving the caret or an end of the
/// selection: at the finger, over the caret's place, or put away (`None`).
#[cfg(target_os = "ios")]
pub fn loupe(window: &gpui::Window, at: Option<(gpui::Point<gpui::Pixels>, gpui::Bounds<gpui::Pixels>)>) {
    text_touch::loupe(window, at);
}
#[cfg(not(target_os = "ios"))]
pub fn loupe(_: &gpui::Window, _: Option<(gpui::Point<gpui::Pixels>, gpui::Bounds<gpui::Pixels>)>) {}

/// Open a file or a link with the app the system gives it. On an iPhone
/// and an iPad an address opens in its app (a page in Safari), and a file
/// in the system's own viewer over the whole window (`look::show`, Quick
/// Look: a PDF, a document, a picture, a sound), which has the way to
/// share it with another app.
#[cfg(not(target_os = "ios"))]
pub fn open(target: &str) {
    let _ = opener::open(target);
}
#[cfg(target_os = "ios")]
pub fn open(target: &str) {
    use objc2::MainThreadMarker;
    use objc2_foundation::{NSDictionary, NSString, NSURL};
    let Some(mtm) = MainThreadMarker::new() else { return };
    if !target.contains("://") && !target.starts_with("mailto:") {
        look::show(std::path::Path::new(target), mtm);
        return;
    }
    let Some(url) = NSURL::URLWithString(&NSString::from_str(target)) else { return };
    if url.scheme().is_none() {
        return;
    }
    // SAFETY: a URL and an empty dictionary of options, on the main thread.
    unsafe { objc2_ui_kit::UIApplication::sharedApplication(mtm).openURL_options_completionHandler(&url, &NSDictionary::new(), None) };
}

/// The system's viewer for a file on an iPhone and an iPad.
#[cfg(target_os = "ios")]
mod look {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, NSObject, NSObjectProtocol};
    use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
    use objc2_foundation::{NSString, NSURL};

    #[link(name = "QuickLook", kind = "framework")]
    unsafe extern "C" {}

    define_class!(
        // What the viewer asks for its one file.
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        #[name = "OnyxLookSource"]
        #[ivars = Retained<NSURL>]
        struct Source;

        unsafe impl NSObjectProtocol for Source {}

        impl Source {
            #[unsafe(method(numberOfPreviewItemsInPreviewController:))]
            fn count(&self, _viewer: &AnyObject) -> isize {
                1
            }

            #[unsafe(method_id(previewController:previewItemAtIndex:))]
            fn item(&self, _viewer: &AnyObject, _at: isize) -> Retained<NSURL> {
                self.ivars().clone()
            }
        }
    );

    thread_local! {
        /// The viewer keeps its source weakly: the last one is kept here.
        static KEPT: std::cell::RefCell<Option<Retained<Source>>> = const { std::cell::RefCell::new(None) };
    }

    /// The controller on top of the window that has the keyboard.
    pub(super) fn top(mtm: MainThreadMarker) -> Option<Retained<AnyObject>> {
        let app = objc2_ui_kit::UIApplication::sharedApplication(mtm);
        // SAFETY: UIKit's own objects asked for what each has, on the main thread; a scene is asked for its key window only when it answers it (a window scene does).
        unsafe {
            let scenes: Retained<AnyObject> = msg_send![&*app, connectedScenes];
            let scenes: Retained<AnyObject> = msg_send![&*scenes, allObjects];
            let count: usize = msg_send![&*scenes, count];
            let mut window: Option<Retained<AnyObject>> = None;
            for i in 0..count {
                let scene: Retained<AnyObject> = msg_send![&*scenes, objectAtIndex: i];
                let answers: bool = msg_send![&*scene, respondsToSelector: sel!(keyWindow)];
                if answers {
                    window = msg_send![&*scene, keyWindow];
                    if window.is_some() {
                        break;
                    }
                }
            }
            let mut top: Retained<AnyObject> = msg_send![&*window?, rootViewController];
            loop {
                let over: Option<Retained<AnyObject>> = msg_send![&*top, presentedViewController];
                match over {
                    Some(over) => top = over,
                    None => return Some(top),
                }
            }
        }
    }

    pub(super) fn show(path: &std::path::Path, mtm: MainThreadMarker) {
        let (Some(viewer), Some(top)) = (AnyClass::get(c"QLPreviewController"), top(mtm)) else { return };
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let source = Source::alloc(mtm).set_ivars(url);
        // SAFETY: NSObject's own init; the viewer is made, given its source (kept alive in `KEPT`), told to take the whole screen (UIModalPresentationFullScreen is 0) and shown by the controller on top, all on the main thread.
        unsafe {
            let source: Retained<Source> = msg_send![super(source), init];
            let viewer: Retained<AnyObject> = msg_send![viewer, new];
            let _: () = msg_send![&*viewer, setDataSource: &*source];
            let _: () = msg_send![&*viewer, setModalPresentationStyle: 0isize];
            KEPT.with(|k| *k.borrow_mut() = Some(source));
            let _: () = msg_send![&*top, presentViewController: &*viewer, animated: true, completion: std::ptr::null::<AnyObject>()];
        }
    }
}

/// A picture chosen in the system's own picker of the photos, written to
/// a file of its own (a PNG, turned as it is seen, its short side at most
/// 1024 points): the answer is that file, or nothing when the picker is
/// let go. The picker needs no leave to read the library: it hands over
/// the one picture chosen. An iPhone's and an iPad's; elsewhere nothing.
#[cfg(target_os = "ios")]
pub fn pick_picture(to: std::path::PathBuf) -> futures::channel::oneshot::Receiver<Option<std::path::PathBuf>> {
    let (tx, rx) = futures::channel::oneshot::channel();
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        photos::show(to, tx, mtm);
    }
    rx
}

#[cfg(target_os = "ios")]
mod photos {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use block2::RcBlock;
    use futures::channel::oneshot::Sender;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, NSObject, NSObjectProtocol};
    use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
    use objc2_core_foundation::CGSize;
    use objc2_foundation::{NSArray, NSError};
    use objc2_ui_kit::UIImage;

    #[link(name = "PhotosUI", kind = "framework")]
    unsafe extern "C" {}

    type Answer = (PathBuf, Sender<Option<PathBuf>>);

    define_class!(
        // Told what the picker chose.
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        #[name = "OnyxPhotoPicker"]
        #[ivars = RefCell<Option<Answer>>]
        struct Picker;

        unsafe impl NSObjectProtocol for Picker {}

        impl Picker {
            #[unsafe(method(picker:didFinishPicking:))]
            fn picked(&self, picker: &AnyObject, results: &NSArray) {
                // SAFETY: the picker is a view controller, let go on the main thread.
                let _: () = unsafe { msg_send![picker, dismissViewControllerAnimated: true, completion: std::ptr::null::<AnyObject>()] };
                let Some((to, tx)) = self.ivars().borrow_mut().take() else { return };
                let Some(first) = results.firstObject() else {
                    let _ = tx.send(None);
                    return;
                };
                let Some(image_class) = AnyClass::get(c"UIImage") else {
                    let _ = tx.send(None);
                    return;
                };
                // The answer goes once, from whichever queue the provider answers on.
                let answer = Mutex::new(Some((to, tx)));
                let done = RcBlock::new(move |object: *mut AnyObject, _error: *mut NSError| {
                    let Some((to, tx)) = answer.lock().ok().and_then(|mut a| a.take()) else { return };
                    // SAFETY: the provider hands a UIImage, as it was asked for one, or nothing.
                    let image = unsafe { object.as_ref() }.and_then(|o| o.downcast_ref::<UIImage>());
                    let _ = tx.send(image.is_some_and(|image| keep(image, &to)).then_some(to));
                });
                // SAFETY: a picker's result has an item provider, asked for a UIImage with a block that lives as long as the call needs it.
                unsafe {
                    let provider: Retained<AnyObject> = msg_send![&*first, itemProvider];
                    let _: Retained<AnyObject> = msg_send![&*provider, loadObjectOfClass: image_class, completionHandler: &*done];
                }
            }
        }
    );

    thread_local! {
        static KEPT: RefCell<Option<Retained<Picker>>> = const { RefCell::new(None) };
    }

    /// The picture drawn as it is seen, its short side at most 1024
    /// points, written as a PNG.
    fn keep(image: &UIImage, to: &std::path::Path) -> bool {
        // SAFETY: a picture's size, read where it was handed over.
        let size = unsafe { image.size() };
        let short = size.width.min(size.height).max(1.);
        let k = (1024. / short).min(1.);
        // SAFETY: UIKit's own drawing of a picture at a size, safe off the main thread since iOS 15.
        let small: Option<Retained<UIImage>> = unsafe { msg_send![image, imageByPreparingThumbnailOfSize: CGSize::new(size.width * k, size.height * k)] };
        let Some(png) = small.as_deref().unwrap_or(image).png_representation() else { return false };
        std::fs::write(to, png.to_vec()).is_ok()
    }

    pub(super) fn show(to: PathBuf, tx: Sender<Option<PathBuf>>, mtm: MainThreadMarker) {
        let (Some(config_class), Some(filter_class), Some(picker_class), Some(top)) = (AnyClass::get(c"PHPickerConfiguration"), AnyClass::get(c"PHPickerFilter"), AnyClass::get(c"PHPickerViewController"), super::look::top(mtm)) else {
            let _ = tx.send(None);
            return;
        };
        let delegate = Picker::alloc(mtm).set_ivars(RefCell::new(Some((to, tx))));
        // SAFETY: PhotosUI's own classes made and set as its header has them, on the main thread; the delegate is kept in `KEPT`, as the picker keeps it weakly.
        unsafe {
            let delegate: Retained<Picker> = msg_send![super(delegate), init];
            let config: Retained<AnyObject> = msg_send![config_class, new];
            let filter: Retained<AnyObject> = msg_send![filter_class, imagesFilter];
            let _: () = msg_send![&*config, setFilter: &*filter];
            let _: () = msg_send![&*config, setSelectionLimit: 1isize];
            let picker: Retained<AnyObject> = msg_send![msg_send![picker_class, alloc], initWithConfiguration: &*config];
            let _: () = msg_send![&*picker, setDelegate: &*delegate];
            KEPT.with(|k| *k.borrow_mut() = Some(delegate));
            let _: () = msg_send![&*top, presentViewController: &*picker, animated: true, completion: std::ptr::null::<AnyObject>()];
        }
    }
}

/// The frame's plates of glass (`glass.rs`) laid over gpui's view as
/// views of UIKit's, each with its icons: made as a plate is first named,
/// moved and faded as the frame says, and taken away once no frame names
/// it. On iOS 26 and later the plate is Liquid Glass (`UIGlassEffect`);
/// before it, the system's thin material, which blurs as glass did then.
/// Nothing elsewhere.
#[cfg(not(target_os = "ios"))]
pub fn glass(_window: &gpui::Window, _cx: &gpui::App, _dark: bool, _plates: &[crate::glass::Plate]) {}
#[cfg(target_os = "ios")]
pub fn glass(window: &gpui::Window, cx: &gpui::App, dark: bool, plates: &[crate::glass::Plate]) {
    panes::lay(window, cx, dark, plates);
}

#[cfg(target_os = "ios")]
mod panes {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use gpui::{Bounds, Hsla, Pixels, Point, SharedString};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool, NSObject, NSObjectProtocol};
    use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_foundation::{NSData, NSString};
    use objc2_ui_kit::{UIBlurEffect, UIBlurEffectStyle, UIColor, UICornerConfiguration, UICornerRadius, UIGlassEffect, UIGlassEffectStyle, UIImage, UIImageRenderingMode, UIImageView, UIUserInterfaceStyle, UIView, UIViewContentMode, UIVisualEffect, UIVisualEffectView};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use crate::glass::{FieldEvent, Mark, Plate};

    define_class!(
        // What a field on a plate says, told to the window (`glass::field_says`).
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        #[name = "OnyxGlassField"]
        #[ivars = &'static str]
        struct FieldTarget;

        unsafe impl NSObjectProtocol for FieldTarget {}

        impl FieldTarget {
            #[unsafe(method(changed:))]
            fn changed(&self, field: &AnyObject) {
                // SAFETY: the sender is the UITextField this target was added to.
                let words: Option<Retained<NSString>> = unsafe { msg_send![field, text] };
                crate::glass::field_says(FieldEvent::Changed(self.ivars(), words.map(|w| w.to_string()).unwrap_or_default()));
            }

            #[unsafe(method(textFieldShouldReturn:))]
            fn returned(&self, _field: &AnyObject) -> Bool {
                crate::glass::field_says(FieldEvent::Return(self.ivars()));
                Bool::NO
            }

            #[unsafe(method(textFieldDidBeginEditing:))]
            fn began(&self, _field: &AnyObject) {
                crate::glass::field_says(FieldEvent::Focus(self.ivars(), true));
            }

            #[unsafe(method(textFieldDidEndEditing:))]
            fn ended(&self, _field: &AnyObject) {
                crate::glass::field_says(FieldEvent::Focus(self.ivars(), false));
            }
        }
    );

    /// A text field's input trait set: UIKit forwards these to an object of
    /// its own, so the field itself has no method of the name for objc2's
    /// check of a message, and the message is sent bare.
    ///
    /// # Safety
    /// `view` is a UITextField and `sel` one of its traits' setters of an `NSInteger`.
    unsafe fn trait_to(view: &UIView, sel: objc2::runtime::Sel, value: isize) {
        let send: unsafe extern "C-unwind" fn(*const UIView, objc2::runtime::Sel, isize) = unsafe { std::mem::transmute(objc2::ffi::objc_msgSend as *const ()) };
        unsafe { send(view, sel, value) };
    }

    /// What kind of view a sign is.
    #[derive(PartialEq, Clone, Copy)]
    enum Kind {
        Picture,
        Field,
        Label,
    }

    fn kind_of(m: &Mark) -> Kind {
        if m.field.is_some() {
            Kind::Field
        } else if m.label.is_some() {
            Kind::Label
        } else {
            Kind::Picture
        }
    }

    /// A plate's view and what it was last told, so a frame that says
    /// the same tells the system nothing.
    struct Pane {
        view: Retained<UIVisualEffectView>,
        marks: Vec<Sign>,
        at: Option<CGRect>,
        round: f32,
        show: f32,
        dark: Option<bool>,
        pressed: bool,
        /// When the press began, so a tap's swell is seen before it goes.
        pressed_at: Option<std::time::Instant>,
    }

    struct Sign {
        view: Retained<UIView>,
        kind: Kind,
        picture: Option<Retained<UIImageView>>,
        target: Option<Retained<FieldTarget>>,
        set: u64,
        focus: u64,
        words: Option<SharedString>,
        icon: Option<SharedString>,
        at: Option<CGRect>,
        ink: Option<Hsla>,
        round: f32,
        show: f32,
        pressed: bool,
        pressed_at: Option<std::time::Instant>,
    }

    thread_local! {
        static PANES: RefCell<HashMap<&'static str, Pane>> = RefCell::new(HashMap::new());
        /// The plates in the order they were last stacked, as gpui painted them.
        static STACK: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
        /// Each icon as the system draws it, read once from its file.
        static ICONS: RefCell<HashMap<SharedString, Option<Retained<UIImage>>>> = RefCell::new(HashMap::new());
    }

    fn rect(b: Bounds<Pixels>, from: Point<Pixels>) -> CGRect {
        CGRect::new(CGPoint::new(f64::from(f32::from(b.origin.x - from.x)), f64::from(f32::from(b.origin.y - from.y))), CGSize::new(f64::from(f32::from(b.size.width)), f64::from(f32::from(b.size.height))))
    }

    fn color(ink: Hsla) -> Retained<UIColor> {
        let c = gpui::Rgba::from(ink);
        UIColor::colorWithRed_green_blue_alpha(c.r as f64, c.g as f64, c.b as f64, c.a as f64)
    }

    /// An icon of the app's as a picture the system inks itself: its
    /// shape alone, drawn large enough for any screen.
    fn icon(path: &SharedString, cx: &gpui::App) -> Option<Retained<UIImage>> {
        ICONS.with(|icons| {
            icons
                .borrow_mut()
                .entry(path.clone())
                .or_insert_with(|| {
                    let bytes = cx.asset_source().load(path).ok()??;
                    let drawn = cx.svg_renderer().render_single_frame(&bytes, 3.).ok()?;
                    let size = drawn.size(0);
                    let (w, h) = (size.width.0 as u32, size.height.0 as u32);
                    let shape: Vec<u8> = drawn.as_bytes(0)?.chunks_exact(4).flat_map(|p| [0, 0, 0, p[3]]).collect();
                    let mut png = std::io::Cursor::new(Vec::new());
                    image::RgbaImage::from_raw(w, h, shape)?.write_to(&mut png, image::ImageFormat::Png).ok()?;
                    let picture = UIImage::imageWithData(&NSData::with_bytes(png.get_ref()))?;
                    Some(picture.imageWithRenderingMode(UIImageRenderingMode::AlwaysTemplate))
                })
                .clone()
        })
    }

    fn effect(mtm: MainThreadMarker, liquid: bool) -> Retained<UIVisualEffect> {
        if liquid {
            // The system's own glass answers a finger as its buttons' does: it
            // swells and lights under a press, a tap as much as a hold.
            let glass = UIGlassEffect::effectWithStyle(UIGlassEffectStyle::Regular, mtm);
            glass.setInteractive(true);
            Retained::into_super(glass)
        } else {
            Retained::into_super(UIBlurEffect::effectWithStyle(UIBlurEffectStyle::SystemThinMaterial, mtm))
        }
    }

    /// How long a press's swell takes to come whole, in seconds: a tap's
    /// lift waits for it.
    const SWELL: f64 = 0.16;

    /// UIKit's animation of what `change` does to views, with a spring.
    fn spring(seconds: f64, damping: f64, change: impl Fn() + 'static, done: Option<Box<dyn Fn()>>) {
        spring_after(0., seconds, damping, change, done);
    }

    /// `spring`, begun after `delay` seconds.
    fn spring_after(delay: f64, seconds: f64, damping: f64, change: impl Fn() + 'static, done: Option<Box<dyn Fn()>>) {
        let change = block2::RcBlock::new(change);
        let done = block2::RcBlock::new(move |_: Bool| {
            if let Some(done) = done.as_ref() {
                done();
            }
        });
        // SAFETY: UIView's class method with two blocks that live through the call (UIKit copies them); allowing presses (1 << 1) as the views move and going on from where a view is (1 << 2), on the main thread.
        unsafe {
            let _: () = msg_send![<UIView as objc2::ClassType>::class(), animateWithDuration: seconds, delay: delay, usingSpringWithDamping: damping, initialSpringVelocity: 0.0f64, options: 6usize, animations: &*change, completion: &*done];
        }
    }

    fn make(host: &UIView, mtm: MainThreadMarker, liquid: bool) -> Pane {
        // Liquid Glass comes as the system's own glass does, condensing out
        // of nothing (its effect set in an animation); the material before
        // it is simply there.
        let material = (!liquid).then(|| effect(mtm, false));
        let view = UIVisualEffectView::initWithEffect(UIVisualEffectView::alloc(mtm), material.as_deref());
        if liquid {
            let (glass, view) = (effect(mtm, true), view.clone());
            spring(0.45, 0.82, move || view.setEffect(Some(&glass)), None);
        }
        // Presses are gpui's: the element under the plate takes them. Liquid
        // Glass is let feel the finger, for its own swell and light; UIKit
        // sends on a touch a view does not use to the next responder, its
        // superview, gpui's view, so the press still reaches the element.
        view.setUserInteractionEnabled(liquid);
        view.contentView().setClipsToBounds(true);
        if !liquid {
            view.setClipsToBounds(true);
        }
        host.addSubview(&view);
        Pane { view, marks: Vec::new(), at: None, round: -1., show: -1., dark: None, pressed: false, pressed_at: None }
    }

    fn sign(pane: &Pane, m: &Mark, mtm: MainThreadMarker) -> Sign {
        let kind = kind_of(m);
        let (view, picture, target): (Retained<UIView>, _, _) = match (kind, &m.field) {
            (Kind::Field, Some(f)) => {
                let target = FieldTarget::alloc(mtm).set_ivars(f.key);
                // SAFETY: NSObject's own init, and a UITextField made and set as UIKit's header has it, on the main thread: the return key Search (6), no correction (1), no capitals (0), no spelling (1), the clear button while editing (1), its words told on each change (UIControlEventEditingChanged, 1 << 17); the target is kept with the sign.
                unsafe {
                    let target: Retained<FieldTarget> = msg_send![super(target), init];
                    let class = AnyClass::get(c"UITextField").expect("UIKit's text field");
                    let field: Retained<UIView> = msg_send![msg_send![class, alloc], initWithFrame: CGRect::ZERO];
                    let font: Retained<AnyObject> = msg_send![AnyClass::get(c"UIFont").expect("UIKit's font"), systemFontOfSize: 17.0f64];
                    let _: () = msg_send![&*field, setFont: &*font];
                    trait_to(&field, sel!(setReturnKeyType:), 6);
                    trait_to(&field, sel!(setAutocorrectionType:), 1);
                    trait_to(&field, sel!(setAutocapitalizationType:), 0);
                    trait_to(&field, sel!(setSpellCheckingType:), 1);
                    let _: () = msg_send![&*field, setClearButtonMode: 1isize];
                    let _: () = msg_send![&*field, addTarget: &*target, action: sel!(changed:), forControlEvents: 1usize << 17];
                    let _: () = msg_send![&*field, setDelegate: &*target];
                    (field, None, Some(target))
                }
            }
            (Kind::Label, _) => {
                // SAFETY: a UILabel made and set on the main thread, its words at the right (NSTextAlignmentRight, 2).
                unsafe {
                    let class = AnyClass::get(c"UILabel").expect("UIKit's label");
                    let label: Retained<UIView> = msg_send![msg_send![class, alloc], initWithFrame: CGRect::ZERO];
                    let font: Retained<AnyObject> = msg_send![AnyClass::get(c"UIFont").expect("UIKit's font"), monospacedDigitSystemFontOfSize: 15.0f64, weight: 0.0f64];
                    let _: () = msg_send![&*label, setFont: &*font];
                    let _: () = msg_send![&*label, setTextAlignment: 2isize];
                    label.setUserInteractionEnabled(false);
                    (label, None, None)
                }
            }
            _ => {
                let picture = UIImageView::initWithFrame(UIImageView::alloc(mtm), CGRect::ZERO);
                picture.setContentMode(UIViewContentMode::ScaleAspectFit);
                picture.setUserInteractionEnabled(false);
                (Retained::into_super(picture.clone()), Some(picture), None)
            }
        };
        pane.view.contentView().addSubview(&view);
        Sign { view, kind, picture, target, set: u64::MAX, focus: 0, words: None, icon: None, at: None, ink: None, round: -1., show: -1., pressed: false, pressed_at: None }
    }

    fn tell(s: &mut Sign, m: &Mark, from: Point<Pixels>, cx: &gpui::App) {
        let at = rect(m.at, from);
        if s.at != Some(at) {
            // By its bounds and center, which hold under a press's scale.
            s.view.setBounds(CGRect::new(CGPoint::ZERO, at.size));
            s.view.setCenter(CGPoint::new(at.origin.x + at.size.width / 2., at.origin.y + at.size.height / 2.));
            s.at = Some(at);
        }
        // The icon under a finger rises a little in the glass, at once, and
        // settles back on a spring once it has come whole.
        if s.pressed != m.pressed {
            s.pressed = m.pressed;
            let held = s.pressed_at.map_or(1., |t| t.elapsed().as_secs_f64());
            s.pressed_at = m.pressed.then(std::time::Instant::now);
            let delay = if m.pressed { 0. } else { (SWELL - held).max(0.) };
            let grow = if m.pressed { 1.2 } else { 1. };
            let view = s.view.clone();
            spring_after(delay, if m.pressed { 0.22 } else { 0.5 }, if m.pressed { 0.85 } else { 0.5 }, move || {
                // SAFETY: a view's transform set to a scale, in UIKit's animation, on the main thread.
                let _: () = unsafe { msg_send![&*view, setTransform: objc2_core_foundation::CGAffineTransform { a: grow, b: 0., c: 0., d: grow, tx: 0., ty: 0. }] };
            }, None);
        }
        match s.kind {
            Kind::Picture => {
                if let Some(picture) = s.picture.as_ref().filter(|_| s.icon != m.icon || s.ink != Some(m.ink)) {
                    match &m.icon {
                        Some(path) => {
                            picture.setImage(icon(path, cx).as_deref());
                            // SAFETY: a view's own ink set to a color, on the main thread.
                            unsafe { picture.setTintColor(Some(&color(m.ink))) };
                            picture.setBackgroundColor(None);
                        }
                        None => {
                            picture.setImage(None);
                            picture.setBackgroundColor(Some(&color(m.ink)));
                        }
                    }
                    (s.icon, s.ink) = (m.icon.clone(), Some(m.ink));
                }
            }
            Kind::Field => {
                let Some(f) = m.field.as_ref() else { return };
                // SAFETY: the sign's view is a UITextField, told on the main thread.
                unsafe {
                    if s.ink != Some(m.ink) {
                        let _: () = msg_send![&*s.view, setTextColor: &*color(m.ink)];
                        let _: () = msg_send![&*s.view, setPlaceholder: &*NSString::from_str(&f.placeholder)];
                        s.ink = Some(m.ink);
                    }
                    if s.set != f.set.0 {
                        let _: () = msg_send![&*s.view, setText: &*NSString::from_str(&f.set.1)];
                        s.set = f.set.0;
                    }
                    if s.focus != f.focus.0 {
                        s.focus = f.focus.0;
                        if f.focus.1 {
                            let _: Bool = msg_send![&*s.view, becomeFirstResponder];
                        } else {
                            let _: Bool = msg_send![&*s.view, resignFirstResponder];
                        }
                    }
                }
            }
            Kind::Label => {
                if s.words != m.label || s.ink != Some(m.ink) {
                    // SAFETY: the sign's view is a UILabel, told on the main thread.
                    unsafe {
                        let _: () = msg_send![&*s.view, setText: &*NSString::from_str(m.label.as_deref().unwrap_or(""))];
                        let _: () = msg_send![&*s.view, setTextColor: &*color(m.ink)];
                    }
                    (s.words, s.ink) = (m.label.clone(), Some(m.ink));
                }
            }
        }
        if s.round != m.round {
            s.view.layer().setCornerRadius(f64::from(m.round.min(f32::from(m.at.size.height) / 2.)));
            s.view.setClipsToBounds(m.round > 0.);
            s.round = m.round;
        }
        if s.show != m.show {
            s.view.setAlpha(f64::from(m.show));
            s.show = m.show;
        }
    }

    pub(super) fn lay(window: &gpui::Window, cx: &gpui::App, dark: bool, plates: &[Plate]) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
        let RawWindowHandle::UiKit(handle) = handle.as_raw() else { return };
        // SAFETY: the handle's view is the window's own UIView, alive while the window is, and this is the main thread.
        let host: &UIView = unsafe { handle.ui_view.cast().as_ref() };
        let liquid = AnyClass::get(c"UIGlassEffect").is_some();
        let shown = |p: &&Plate| p.show > 0.004 && p.at.size.width > gpui::px(1.);
        PANES.with(|panes| {
            let mut panes = panes.borrow_mut();
            panes.retain(|key, pane| {
                let keep = plates.iter().filter(shown).any(|p| p.key == *key);
                if !keep {
                    // A field on it lets the keyboard go now, as the plate
                    // starts to go, not when the melt below has ended.
                    for s in pane.marks.iter().filter(|s| s.kind == Kind::Field) {
                        // SAFETY: the sign's view is a UITextField, told on the main thread.
                        let _: Bool = unsafe { msg_send![&*s.view, resignFirstResponder] };
                    }
                    // A plate that goes while it shows melts back into what
                    // is behind it, as the system's glass does; one that had
                    // faded out goes now.
                    if liquid && pane.show > 0.5 {
                        let (view, gone) = (pane.view.clone(), pane.view.clone());
                        for s in &pane.marks {
                            let mark = s.view.clone();
                            spring(0.3, 1.0, move || mark.setAlpha(0.), None);
                        }
                        spring(0.35, 1.0, move || view.setEffect(None), Some(Box::new(move || gone.removeFromSuperview())));
                    } else {
                        pane.view.removeFromSuperview();
                    }
                }
                keep
            });
            for p in plates.iter().filter(shown) {
                let pane = panes.entry(p.key).or_insert_with(|| make(host, mtm, liquid));
                let at = rect(p.at, Point::default());
                if pane.at != Some(at) {
                    // By its bounds and center, which hold under a press's
                    // scale; a frame does not.
                    pane.view.setBounds(CGRect::new(CGPoint::ZERO, at.size));
                    pane.view.setCenter(CGPoint::new(at.origin.x + at.size.width / 2., at.origin.y + at.size.height / 2.));
                    pane.at = Some(at);
                }
                if pane.round != p.round {
                    let round = f64::from(p.round.min(f32::from(p.at.size.height) / 2.));
                    if liquid {
                        pane.view.setCornerConfiguration(&UICornerConfiguration::configurationWithRadius(&UICornerRadius::fixedRadius(round)));
                    } else {
                        pane.view.layer().setCornerRadius(round);
                    }
                    pane.view.contentView().layer().setCornerRadius(round);
                    pane.round = p.round;
                }
                if pane.show != p.show {
                    pane.view.setAlpha(f64::from(p.show));
                    pane.show = p.show;
                }
                // A press swells the plate, more for a small one, and lets it
                // back on a spring as the finger lifts. A tap's lift comes
                // a few frames after its landing, so the plate is let swell
                // whole first, as the system's buttons do under a tap.
                if pane.pressed != p.pressed {
                    pane.pressed = p.pressed;
                    let held = pane.pressed_at.map_or(1., |t| t.elapsed().as_secs_f64());
                    pane.pressed_at = p.pressed.then(std::time::Instant::now);
                    let delay = if p.pressed { 0. } else { (SWELL - held).max(0.) };
                    if crate::editor::tracing() {
                        eprintln!("trace glass pressed {} {}", p.key, p.pressed);
                    }
                    let grow = if p.pressed { 1. + (6. / f64::from(f32::from(p.at.size.width).max(1.))).min(0.12) } else { 1. };
                    let view = pane.view.clone();
                    spring_after(delay, if p.pressed { 0.25 } else { 0.5 }, if p.pressed { 0.9 } else { 0.55 }, move || {
                        // SAFETY: a view's transform set to a scale, in UIKit's animation, on the main thread.
                        let _: () = unsafe { msg_send![&*view, setTransform: objc2_core_foundation::CGAffineTransform { a: grow, b: 0., c: 0., d: grow, tx: 0., ty: 0. }] };
                    }, None);
                }
                let dark = p.dark.unwrap_or(dark);
                if pane.dark != Some(dark) {
                    pane.view.setOverrideUserInterfaceStyle(if dark { UIUserInterfaceStyle::Dark } else { UIUserInterfaceStyle::Light });
                    pane.dark = Some(dark);
                }
                // A sign of another kind in a mark's place is made again.
                let same = pane.marks.iter().zip(&p.marks).take_while(|(s, m)| s.kind == kind_of(m) && s.target.as_ref().map(|t| *t.ivars()) == m.field.as_ref().map(|f| f.key)).count();
                while pane.marks.len() > same {
                    if let Some(gone) = pane.marks.pop() {
                        gone.view.removeFromSuperview();
                    }
                }
                while pane.marks.len() < p.marks.len() {
                    let made = sign(pane, &p.marks[pane.marks.len()], mtm);
                    pane.marks.push(made);
                }
                // A plate with a field takes the finger: the field is pressed
                // where it is, and a touch on the rest of the plate goes on
                // to gpui's view, the next responder.
                pane.view.setUserInteractionEnabled(liquid || p.marks.iter().any(|m| m.field.is_some()));
                for (s, m) in pane.marks.iter_mut().zip(&p.marks) {
                    tell(s, m, p.at.origin, cx);
                }
            }
            // The views stack as gpui painted their plates, a plate painted
            // later over one painted before; told only when that changes.
            STACK.with(|stack| {
                if !stack.borrow().iter().copied().eq(plates.iter().filter(shown).map(|p| p.key)) {
                    let order: Vec<&'static str> = plates.iter().filter(shown).map(|p| p.key).collect();
                    for key in &order {
                        if let Some(pane) = panes.get(key) {
                            host.bringSubviewToFront(&pane.view);
                        }
                    }
                    *stack.borrow_mut() = order;
                }
            });
        });
    }
}

/// Show `path` selected in the file manager. macOS and Windows can select
/// a file; on Linux the folder opens, as close as xdg gets without D-Bus.
/// A path as a person reads it: a path in iCloud Drive from `iCloud
/// Drive`, as the Finder names the folder, Onyx's own library in it as
/// `iCloud Drive/Onyx`, the home folder as `~`, and
/// the system's own separator. The same on every system but the
/// separator and where iCloud Drive is.
pub fn short_path(path: &std::path::Path) -> String {
    let home = onyx_core::paths::home();
    let sep = std::path::MAIN_SEPARATOR;
    let cloud = home.join("Library").join("Mobile Documents").join("com~apple~CloudDocs");
    // Onyx's own iCloud library, which the Finder shows in iCloud Drive as Onyx.
    let library = home.join("Library").join("Mobile Documents").join("iCloud~com~alkalinesoftware~onyx").join("Documents");
    if let Some(rest) = icloud_container().into_iter().chain([library]).find_map(|lib| path.strip_prefix(&lib).ok().map(std::path::Path::to_path_buf)) {
        return if rest.as_os_str().is_empty() { format!("iCloud Drive{sep}Onyx") } else { format!("iCloud Drive{sep}Onyx{sep}{}", rest.display()) };
    }
    if let Ok(rest) = path.strip_prefix(&cloud) {
        return if rest.as_os_str().is_empty() { "iCloud Drive".to_string() } else { format!("iCloud Drive{sep}{}", rest.display()) };
    }
    // An iPhone's own folder for the app, as Files names it.
    if cfg!(target_os = "ios") {
        if let Ok(rest) = path.strip_prefix(onyx_core::paths::documents()) {
            static PAD: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            let device = crate::i18n::t(if *PAD.get_or_init(is_pad) { phrase("On My iPad") } else { phrase("On My iPhone") });
            return if rest.as_os_str().is_empty() { format!("{device}{sep}Onyx") } else { format!("{device}{sep}Onyx{sep}{}", rest.display()) };
        }
    }
    match path.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~{sep}{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// Whether this is an iPad, for what is named after the device.
#[cfg(target_os = "ios")]
pub fn is_pad() -> bool {
    use objc2_ui_kit::{UIDevice, UIUserInterfaceIdiom};
    objc2::MainThreadMarker::new().is_some_and(|mtm| UIDevice::currentDevice(mtm).userInterfaceIdiom() == UIUserInterfaceIdiom::Pad)
}
#[cfg(not(target_os = "ios"))]
pub fn is_pad() -> bool {
    false
}

/// Whether one file is open at a time, with no tabs to keep others in:
/// on an iPhone, as its own apps have it. An iPad has the Mac's tabs.
pub fn one_file() -> bool {
    static PHONE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    cfg!(target_os = "ios") && *PHONE.get_or_init(|| !is_pad())
}

/// A key binding as this system has it. `secondary-` is ⌘ on a Mac, an
/// iPhone and an iPad, and Ctrl elsewhere. gpui's own `KeyBinding::new`
/// reads no `secondary-`, and its `load` gives an iPad's keyboard Ctrl.
pub fn key<A: gpui::Action>(keys: &str, action: A, context: Option<&str>, cx: &gpui::App) -> gpui::KeyBinding {
    let secondary = if cfg!(any(target_os = "macos", target_os = "ios")) { "cmd-" } else { "ctrl-" };
    let context = context.map(|c| std::rc::Rc::new(gpui::KeyBindingContextPredicate::parse(c).expect("a key's context")));
    gpui::KeyBinding::load(&keys.replace("secondary-", secondary), Box::new(action), context, false, None, cx).expect("a key binding")
}

/// Onyx's own iCloud container, the folder iCloud Drive shows as an app
/// library with the icon on it, when the bundle is signed with the
/// iCloud entitlements and the provisioning profile (`docs/platform.md`).
/// Asked of the system once, off the main thread (`warm_icloud`), since
/// the first answer can take a moment; `None` until then, and for good in
/// a bundle without the entitlements, such as the dev build.
pub fn icloud_container() -> Option<std::path::PathBuf> {
    ICLOUD.get().cloned().flatten()
}

static ICLOUD: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();

/// Ask the system for the container on a thread of its own, at launch.
pub fn warm_icloud() {
    std::thread::Builder::new()
        .name("icloud".into())
        .spawn(|| {
            let _ = ICLOUD.set(icloud_container_now());
            #[cfg(target_os = "ios")]
            read_shared_name();
        })
        .ok();
}

/// The file in Onyx's iCloud container that holds the name a Mac's Onyx
/// greets its person by, for an iPhone's and an iPad's to greet them by
/// too: outside the container's Documents, so iCloud Drive does not show
/// it, and carried to every device as the rest of the container is. A
/// Mac's path is the container's folder, there whether this build has
/// the iCloud entitlements or not.
fn shared_name_file() -> Option<std::path::PathBuf> {
    let container = if cfg!(target_os = "ios") {
        icloud_container()?.parent()?.to_path_buf()
    } else {
        onyx_core::paths::home().join("Library").join("Mobile Documents").join("iCloud~com~alkalinesoftware~onyx")
    };
    container.is_dir().then(|| container.join("Profile").join("name.txt"))
}

/// A Mac's Onyx writes the name it greets by into the container, when it
/// is not there already: at launch and when the name is changed. Off the
/// window's thread; nothing elsewhere.
pub fn share_name(name: String) {
    if !cfg!(target_os = "macos") || name.trim().is_empty() {
        return;
    }
    std::thread::Builder::new()
        .name("share-name".into())
        .spawn(move || {
            let Some(file) = shared_name_file() else { return };
            if std::fs::read_to_string(&file).is_ok_and(|now| now.trim() == name.trim()) {
                return;
            }
            let _ = file.parent().map(std::fs::create_dir_all);
            let _ = onyx_core::paths::write_atomic(&file, name.trim().as_bytes());
        })
        .ok();
}

/// The name a Mac shared, as an iPhone and an iPad have it: asked of
/// iCloud and waited for on the container's thread, a few seconds at most.
#[cfg(target_os = "ios")]
static SHARED_NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();

#[cfg(target_os = "ios")]
fn read_shared_name() {
    let Some(file) = shared_name_file() else { return };
    let until = std::time::Instant::now() + std::time::Duration::from_secs(8);
    if !onyx_core::vault::at_hand(&file) {
        bring(&file);
        while !onyx_core::vault::at_hand(&file) && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    if let Ok(name) = std::fs::read_to_string(&file) {
        let name = name.trim().chars().take(64).collect::<String>();
        if !name.is_empty() {
            let _ = SHARED_NAME.set(name);
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn icloud_container_now() -> Option<std::path::PathBuf> {
    use objc2_foundation::NSFileManager;
    // The container's Documents folder is what iCloud Drive shows; the
    // rest of the container is the app's own.
    let url = NSFileManager::defaultManager().URLForUbiquityContainerIdentifier(None)?;
    let path = url.path()?;
    let dir = std::path::PathBuf::from(path.to_string()).join("Documents");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
fn icloud_container_now() -> Option<std::path::PathBuf> {
    None
}

/// The container on an iPhone and an iPad, where an app reaches no folder
/// but its own and the person chooses none: Onyx's iCloud container, the
/// Mac's own, so a vault made on either is on both; with no iCloud, the
/// app's own Documents, which Files shows as On My iPhone, Onyx. `None`
/// on every other system, where the container is a setting.
#[cfg(target_os = "ios")]
pub fn fixed_container() -> Option<std::path::PathBuf> {
    // Asked as every frame is built: the app's own folder is made and
    // named once, and the iCloud container's answer is kept as it comes.
    static OWN: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    icloud_container().or_else(|| {
        OWN.get_or_init(|| {
            let dir = onyx_core::paths::documents();
            std::fs::create_dir_all(&dir).ok()?;
            Some(dir)
        })
        .clone()
    })
}
#[cfg(not(target_os = "ios"))]
pub fn fixed_container() -> Option<std::path::PathBuf> {
    None
}

/// Ask the system to bring a file's contents to this device, when they
/// are in iCloud and not here (`vault::at_hand`). It answers at once and
/// the file comes in its own time; nothing waits for it. Nothing where
/// no file is kept that way.
pub fn bring(path: &std::path::Path) {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        use objc2_foundation::{NSFileManager, NSString, NSURL};
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        // Refused for a file no iCloud keeps (another provider's): such a
        // file comes when it is read, off the window's thread.
        let _ = NSFileManager::defaultManager().startDownloadingUbiquitousItemAtURL_error(&url);
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    let _ = path;
}

/// What another device writes to Onyx's iCloud container, followed while
/// the app runs: on an iPhone and an iPad iCloud tells the device a file
/// changed elsewhere and leaves the old copy in place until it is asked
/// for, so a note edited on the Mac never came while it stayed open. A
/// query of the container's (`NSMetadataQuery`) hears each change, and
/// every file of it that is not the newest is asked for, unless it is
/// coming already (a running query has iCloud bring what changes); it
/// lands in place, which no folder's event tells, so once it is the
/// newest the window has the vault looked over (`icloud_landed`) and the
/// open note takes it as it takes any write from outside. Started once,
/// on the main thread; nothing on a Mac, whose iCloud brings every change
/// itself.
#[cfg(target_os = "ios")]
pub fn follow_icloud() {
    follow::start();
}
#[cfg(not(target_os = "ios"))]
pub fn follow_icloud() {}

/// Told each time a file iCloud was bringing has come: it lands in place,
/// which no folder's event tells (`onyx_core::watch`), so the window has
/// the vault looked over again. One listener, the window's.
pub fn icloud_landed() -> futures::channel::mpsc::UnboundedReceiver<()> {
    let (tx, rx) = futures::channel::mpsc::unbounded();
    LANDED.with(|l| *l.borrow_mut() = Some(tx));
    rx
}

thread_local! {
    static LANDED: std::cell::RefCell<Option<futures::channel::mpsc::UnboundedSender<()>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(target_os = "ios")]
mod follow {
    use std::cell::RefCell;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
    use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
    use objc2_foundation::{NSArray, NSMetadataItem, NSMetadataItemPathKey, NSMetadataQuery, NSMetadataQueryDidFinishGatheringNotification, NSMetadataQueryDidUpdateNotification, NSMetadataQueryUbiquitousDocumentsScope, NSMetadataQueryUpdateChangedItemsKey, NSMetadataUbiquitousItemDownloadingStatusCurrent, NSMetadataUbiquitousItemDownloadingStatusKey, NSMetadataUbiquitousItemIsDownloadingKey, NSNotification, NSNotificationCenter, NSNumber, NSPredicate, NSString};

    define_class!(
        // Told each time the query's results change.
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        #[name = "OnyxCloudFollower"]
        struct Follower;

        unsafe impl NSObjectProtocol for Follower {}

        impl Follower {
            #[unsafe(method(gathered:))]
            fn gathered(&self, note: &NSNotification) {
                // Every file the container holds: each one older here than
                // in iCloud is asked for.
                let Some(query) = note.object().and_then(|o| o.downcast::<NSMetadataQuery>().ok()) else { return };
                if crate::editor::tracing() {
                    eprintln!("trace icloud gathered {}", query.resultCount());
                }
                query.disableUpdates();
                for i in 0..query.resultCount() {
                    if let Ok(item) = query.resultAtIndex(i).downcast::<NSMetadataItem>() {
                        bring_if_old(&item);
                    }
                }
                query.enableUpdates();
            }

            #[unsafe(method(updated:))]
            fn updated(&self, note: &NSNotification) {
                let Some(info) = note.userInfo() else { return };
                // SAFETY: the key is Foundation's own, its value an array of the query's items.
                let Some(changed) = (unsafe { info.objectForKey(NSMetadataQueryUpdateChangedItemsKey) }) else { return };
                let Ok(changed) = changed.downcast::<NSArray>() else { return };
                if crate::editor::tracing() {
                    eprintln!("trace icloud changed {}", changed.count());
                }
                for item in changed.iter() {
                    if let Ok(item) = item.downcast::<NSMetadataItem>() {
                        bring_if_old(&item);
                    }
                }
            }
        }
    );

    thread_local! {
        static KEPT: RefCell<Option<(Retained<NSMetadataQuery>, Retained<Follower>)>> = const { RefCell::new(None) };
        /// The files seen older here than in iCloud, waited for.
        static AWAITED: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
    }

    /// A file whose newest copy is in iCloud and not here, asked for.
    fn bring_if_old(item: &NSMetadataItem) {
        // SAFETY: the keys are Foundation's own constants.
        let (status, path, coming) = unsafe { (item.valueForAttribute(NSMetadataUbiquitousItemDownloadingStatusKey), item.valueForAttribute(NSMetadataItemPathKey), item.valueForAttribute(NSMetadataUbiquitousItemIsDownloadingKey)) };
        let current = status.and_then(|s| s.downcast::<NSString>().ok()).is_some_and(|s| &*s == unsafe { NSMetadataUbiquitousItemDownloadingStatusCurrent });
        let coming = coming.and_then(|c| c.downcast::<NSNumber>().ok()).is_some_and(|c| c.boolValue());
        let Some(path) = path.and_then(|p| p.downcast::<NSString>().ok()).map(|p| p.to_string()) else { return };
        if current {
            // Come: the window looks the vault over for it.
            if AWAITED.with(|a| a.borrow_mut().remove(&path)) {
                if crate::editor::tracing() {
                    eprintln!("trace icloud landed {}", path.rsplit('/').next().unwrap_or_default());
                }
                super::LANDED.with(|l| {
                    if let Some(tx) = l.borrow().as_ref() {
                        let _ = tx.unbounded_send(());
                    }
                });
            }
            return;
        }
        AWAITED.with(|a| a.borrow_mut().insert(path.clone()));
        // Being brought already (the query's own running asks for it), or asked for now.
        if !coming {
            if crate::editor::tracing() {
                eprintln!("trace icloud newer {}", path.rsplit('/').next().unwrap_or_default());
            }
            super::bring(std::path::Path::new(&path));
        }
    }

    pub(super) fn start() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        if KEPT.with(|k| k.borrow().is_some()) {
            return;
        }
        let query = NSMetadataQuery::new();
        // SAFETY: the scope is Foundation's own constant, in an array of strings.
        let scopes = NSArray::from_slice(&[unsafe { NSMetadataQueryUbiquitousDocumentsScope }]);
        // SAFETY: an array of strings is an array of objects.
        unsafe { query.setSearchScopes(Retained::cast_unchecked::<NSArray>(scopes).as_ref()) };
        query.setPredicate(Some(&NSPredicate::predicateWithValue(true)));
        query.setNotificationBatchingInterval(0.5);
        let follower: Retained<Follower> = unsafe { msg_send![Follower::alloc(mtm), init] };
        let center = NSNotificationCenter::defaultCenter();
        // SAFETY: the follower answers both selectors and is kept, with the query, for as long as the app runs.
        unsafe {
            let object: &AnyObject = &query;
            center.addObserver_selector_name_object(&follower, sel!(gathered:), Some(NSMetadataQueryDidFinishGatheringNotification), Some(object));
            center.addObserver_selector_name_object(&follower, sel!(updated:), Some(NSMetadataQueryDidUpdateNotification), Some(object));
        }
        if query.startQuery() {
            KEPT.with(|k| *k.borrow_mut() = Some((query, follower)));
        }
    }
}

/// A vault brought to this device whole, on a thread of its own: every
/// file of it not at hand is asked for, so a note is here before it is
/// pressed and a picture before its note is shown. On an iPhone a vault
/// made on the Mac is names alone at first, and each file read where it
/// was not waited a second and more for the network. One walk at a time,
/// a `lstat` a file; nothing where no file is kept that way.
pub fn bring_vault(root: &std::path::Path) {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        static WALKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if WALKING.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return;
        }
        let root = root.to_path_buf();
        let walk = std::thread::Builder::new().name("bring".into()).spawn(move || {
            let away: Vec<_> = onyx_core::vault::every_file(&root).into_iter().filter(|f| !onyx_core::vault::at_hand(f)).collect();
            if crate::editor::tracing() {
                eprintln!("trace bring {} files not at hand", away.len());
            }
            away.iter().for_each(|f| bring(f));
            WALKING.store(false, std::sync::atomic::Ordering::Release);
        });
        if walk.is_err() {
            WALKING.store(false, std::sync::atomic::Ordering::Release);
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    let _ = root;
}

/// The iCloud container's answer waited for, as long as `most`: on an
/// iPhone the window's first page is the container's vaults, and the
/// answer comes in a moment whether iCloud is set up or not.
pub fn await_icloud(most: std::time::Duration) {
    let until = std::time::Instant::now() + most;
    while ICLOUD.get().is_none() && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// A path kept from an earlier launch, as it is now. On an iPhone and an
/// iPad the app's own folder moves when the app is updated or restored
/// (`…/Containers/Data/Application/<a new id>/`), so a path under an
/// older one is the same place under this one. Elsewhere, the path.
pub fn rehome(path: std::path::PathBuf) -> std::path::PathBuf {
    if !cfg!(target_os = "ios") || path.exists() {
        return path;
    }
    const MARK: &str = "/Containers/Data/Application/";
    let text = path.to_string_lossy();
    let Some(at) = text.find(MARK) else { return path };
    match text[at + MARK.len()..].split_once('/') {
        Some((_, rest)) => onyx_core::paths::home().join(rest),
        None => path,
    }
}

pub fn reveal(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = onyx_core::process::command("open").arg("-R").arg(path).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = onyx_core::process::command("explorer").arg(format!("/select,{}", path.display())).spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if let Some(parent) = path.parent() {
            let _ = opener::open(parent);
        }
    }
}

/// What revealing is called where we are, drawn through `t`.
pub const REVEAL_LABEL: &str = if cfg!(target_os = "macos") {
    phrase("Reveal in Finder")
} else if cfg!(target_os = "windows") {
    phrase("Show in Explorer")
} else {
    phrase("Show in folder")
};

/// What opening a folder in the file manager is called where we are.
pub const OPEN_FOLDER_LABEL: &str = if cfg!(target_os = "macos") {
    phrase("Open in Finder")
} else if cfg!(target_os = "windows") {
    phrase("Open in Explorer")
} else {
    phrase("Open folder")
};

/// The Dock icon, for a bare binary run from `target/`. A bundle carries
/// the same picture as its `.icns`.
/// A picture's pixels, straight RGBA, no larger than `most` on its longer
/// side, turned as the file says it is to be shown. On a Mac the system's
/// ImageIO reads it at that size (a JPEG or HEIC from its smaller scales
/// of itself), never at its own: a photo of 6000 × 6000 decoded whole is
/// 145 MB, which the allocator kept long after a square of 100 was cut
/// from it. Elsewhere it is read whole and made smaller.
#[cfg(target_os = "macos")]
pub fn picture_at_most(path: &std::path::Path, most: u32) -> Option<(u32, u32, Vec<u8>)> {
    picture_io::at_most(path, most).or_else(|| picture_whole(path, most))
}

#[cfg(not(target_os = "macos"))]
pub fn picture_at_most(path: &std::path::Path, most: u32) -> Option<(u32, u32, Vec<u8>)> {
    picture_whole(path, most)
}

fn picture_whole(path: &std::path::Path, most: u32) -> Option<(u32, u32, Vec<u8>)> {
    let picture = image::open(path).ok()?;
    let picture = if picture.width() > most || picture.height() > most { picture.thumbnail(most, most) } else { picture };
    let rgba = picture.into_rgba8();
    let (w, h) = rgba.dimensions();
    Some((w, h, rgba.into_raw()))
}

#[cfg(target_os = "macos")]
mod picture_io {
    use std::ffi::c_void;

    use core_foundation::base::{CFType, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::number::CFNumber;
    use core_foundation::string::{CFString, CFStringRef};
    use core_foundation::url::{CFURL, CFURLRef};
    use core_graphics::base::{kCGBitmapByteOrder32Big, kCGImageAlphaPremultipliedLast};
    use core_graphics::color_space::CGColorSpace;
    use core_graphics::context::CGContext;
    use core_graphics::geometry::{CGPoint, CGRect, CGSize};
    use core_graphics::image::CGImage;
    use foreign_types::ForeignType;

    #[link(name = "ImageIO", kind = "framework")]
    unsafe extern "C" {
        fn CGImageSourceCreateWithURL(url: CFURLRef, options: CFDictionaryRef) -> *mut c_void;
        fn CGImageSourceCreateThumbnailAtIndex(source: *mut c_void, index: usize, options: CFDictionaryRef) -> *mut c_void;
        static kCGImageSourceThumbnailMaxPixelSize: CFStringRef;
        static kCGImageSourceCreateThumbnailFromImageAlways: CFStringRef;
        static kCGImageSourceCreateThumbnailWithTransform: CFStringRef;
        static kCGImageSourceShouldCache: CFStringRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *mut c_void);
    }

    pub fn at_most(path: &std::path::Path, most: u32) -> Option<(u32, u32, Vec<u8>)> {
        let url = CFURL::from_path(path, false)?;
        // SAFETY: the keys are the framework's own constants; the source and
        // the picture are released here, the picture through `CGImage`.
        unsafe {
            let source = CGImageSourceCreateWithURL(url.as_concrete_TypeRef(), std::ptr::null());
            if source.is_null() {
                return None;
            }
            let key = |k: CFStringRef| CFString::wrap_under_get_rule(k);
            let options: CFDictionary<CFString, CFType> = CFDictionary::from_CFType_pairs(&[
                (key(kCGImageSourceThumbnailMaxPixelSize), CFNumber::from(most.max(1) as i64).as_CFType()),
                (key(kCGImageSourceCreateThumbnailFromImageAlways), CFBoolean::true_value().as_CFType()),
                (key(kCGImageSourceCreateThumbnailWithTransform), CFBoolean::true_value().as_CFType()),
                (key(kCGImageSourceShouldCache), CFBoolean::false_value().as_CFType()),
            ]);
            let made = CGImageSourceCreateThumbnailAtIndex(source, 0, options.as_concrete_TypeRef());
            CFRelease(source);
            if made.is_null() {
                return None;
            }
            let picture = CGImage::from_ptr(made as _);
            let (w, h) = (picture.width(), picture.height());
            if w == 0 || h == 0 {
                return None;
            }
            let mut context = CGContext::create_bitmap_context(None, w, h, 8, w * 4, &CGColorSpace::create_device_rgb(), kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big);
            context.draw_image(CGRect::new(&CGPoint::new(0., 0.), &CGSize::new(w as f64, h as f64)), &picture);
            let mut rgba = context.data().to_vec();
            // Straight alpha, as the image crate gives it and the window takes it.
            for px in rgba.chunks_exact_mut(4) {
                let a = px[3] as u32;
                if a != 0 && a != 255 {
                    for c in &mut px[..3] {
                        *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                    }
                }
            }
            Some((w as u32, h as u32, rgba))
        }
    }
}

#[cfg(target_os = "macos")]
pub fn install_dock_icon() {
    use objc2::{AllocAnyThread as _, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;
    let Some(mtm) = MainThreadMarker::new() else { return };
    let data = NSData::with_bytes(include_bytes!("../assets/icon/icon-1024.png"));
    if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
        unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
    }
}
#[cfg(not(target_os = "macos"))]
pub fn install_dock_icon() {}

/// The native window's own ground, under everything the app draws. gpui
/// leaves it the system's, near white in the light appearance, and it
/// showed under a dark frame while the window was drawn or resized. Set
/// to the frame's color, whatever shows there is the frame.
#[cfg(target_os = "macos")]
pub fn set_window_ground(window: &gpui::Window, ground: gpui::Rgba) {
    use objc2_app_kit::NSColor;

    if let Some(native) = ns_view(window).and_then(|v| v.window()) {
        let color = NSColor::colorWithSRGBRed_green_blue_alpha(ground.r as f64, ground.g as f64, ground.b as f64, 1.);
        native.setBackgroundColor(Some(&color));
    }
}
/// On an iPhone and an iPad it is what shows while the screen turns.
#[cfg(target_os = "ios")]
pub fn set_window_ground(window: &gpui::Window, ground: gpui::Rgba) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
    let RawWindowHandle::UiKit(handle) = handle.as_raw() else { return };
    // SAFETY: the handle's view is the window's own UIView, alive while the window is, and this is the main thread.
    let view: &objc2_ui_kit::UIView = unsafe { handle.ui_view.cast().as_ref() };
    let color = objc2_ui_kit::UIColor::colorWithRed_green_blue_alpha(ground.r as f64, ground.g as f64, ground.b as f64, 1.);
    if let Some(native) = view.window() {
        native.setBackgroundColor(Some(&color));
    }
}
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub fn set_window_ground(_window: &gpui::Window, _ground: gpui::Rgba) {}

/// For a probe: words typed as an iPhone's keyboard types them, handed by
/// UIKit's own `insertText:` to the view that has the keyboard, or with no
/// words its delete key (`deleteBackward`). Whether a view took them. This
/// is the whole way a typed key goes, which a probe's `type=` skips by
/// calling the editor. Nowhere else: there a key is posted to the process
/// (`docs/probing.md`).
#[cfg(target_os = "ios")]
pub fn keyboard_types(window: &gpui::Window, words: Option<&str>) -> bool {
    use objc2::rc::Retained;
    use objc2::{msg_send, sel, Message as _};
    use objc2_foundation::NSObjectProtocol as _;
    use objc2_ui_kit::UIView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    fn holder(view: &UIView) -> Option<Retained<UIView>> {
        if view.isFirstResponder() {
            return Some(view.retain());
        }
        view.subviews().iter().find_map(|v| holder(&v))
    }
    let Ok(handle) = HasWindowHandle::window_handle(window) else { return false };
    let RawWindowHandle::UiKit(handle) = handle.as_raw() else { return false };
    // SAFETY: the handle's view is the window's own UIView, alive while the window is, and this is the main thread.
    let view: &UIView = unsafe { handle.ui_view.cast().as_ref() };
    let Some(holder) = view.window().and_then(|w| holder(&w)) else { return false };
    // Sent from the run loop's next turn, as the keyboard sends a key:
    // outside any update of gpui's, which the backend's answer to a key
    // has to make one of its own.
    match words {
        Some(words) if holder.respondsToSelector(sel!(insertText:)) => {
            let words = objc2_foundation::NSString::from_str(words);
            // SAFETY: `insertText:` takes one NSString, which the run loop keeps until it is sent; the view says it answers it.
            let _: () = unsafe { msg_send![&*holder, performSelector: sel!(insertText:), withObject: &*words, afterDelay: 0.0f64] };
            true
        }
        None if holder.respondsToSelector(sel!(deleteBackward)) => {
            // SAFETY: `deleteBackward` takes nothing; the object sent with it is not read.
            let _: () = unsafe { msg_send![&*holder, performSelector: sel!(deleteBackward), withObject: std::ptr::null::<objc2::runtime::AnyObject>(), afterDelay: 0.0f64] };
            true
        }
        _ => false,
    }
}
#[cfg(not(target_os = "ios"))]
pub fn keyboard_types(_window: &gpui::Window, _words: Option<&str>) -> bool {
    false
}

/// The status bar's ink on an iPhone and an iPad, light or dark by what
/// it stands on: the frame is a dark graphite in both appearances, and a
/// compact window's note runs under it on its own paper
/// (`Shell::render`). Asked at every frame, told to the system when it
/// changes. Nothing elsewhere.
pub fn status_bar(light: bool) {
    #[cfg(target_os = "ios")]
    {
        static INK: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(2);
        if INK.swap(light as u8, std::sync::atomic::Ordering::Relaxed) != light as u8 {
            gpui_ios::ios::set_status_bar_style(if light { gpui_ios::StatusBarContentStyle::Light } else { gpui_ios::StatusBarContentStyle::Dark });
        }
    }
    #[cfg(not(target_os = "ios"))]
    let _ = light;
}

/// Where gpui puts the traffic lights' row on the main window: the close
/// button's top left, from the window's top left (`main.rs`).
pub const LIGHTS_ROW: (f32, f32) = (17., 20.);

/// The main window's traffic lights between gpui's row (`k` 1) and a
/// column at the rail's head about `mid` (`k` 0), as the sidebar folds
/// (`Shell::render_rail`). gpui puts them back in its row at a resize,
/// a change of key window or of appearance and more, so each button and
/// their container are watched, and moved again whenever anything moves
/// them. (After emaki's `sys::fold_lights`.)
#[cfg(target_os = "macos")]
pub fn fold_lights(window: &gpui::Window, k: f32, mid: f32) {
    let Some(native) = ns_view(window).and_then(|v| v.window()) else { return };
    lights::FOLD.set((k, mid));
    lights::watch(&native);
    lights::place(&native);
}
#[cfg(not(target_os = "macos"))]
pub fn fold_lights(_: &gpui::Window, _: f32, _: f32) {}

#[cfg(target_os = "macos")]
mod lights {
    use std::cell::{Cell, RefCell};
    use std::ptr::NonNull;

    use objc2::rc::{Retained, Weak};
    use objc2::runtime::ProtocolObject;
    use objc2::Message as _;
    use objc2_app_kit::{NSButton, NSView, NSViewFrameDidChangeNotification, NSWindow, NSWindowButton, NSWindowStyleMask};
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol, NSPoint, NSRect, NSSize};

    thread_local! {
        pub(super) static FOLD: Cell<(f32, f32)> = const { Cell::new((1., 0.)) };
        /// The distance from one button to the next in the system's row,
        /// read while they stand in it.
        static PITCH: Cell<Option<f64>> = const { Cell::new(None) };
        /// Set while the lights are being moved here, so the moves they
        /// post are not answered.
        static MOVING: Cell<bool> = const { Cell::new(false) };
        static WATCHING: RefCell<(usize, Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>)> = const { RefCell::new((0, Vec::new())) };
    }

    fn buttons(native: &NSWindow) -> Option<[Retained<NSButton>; 3]> {
        Some([
            native.standardWindowButton(NSWindowButton::CloseButton)?,
            native.standardWindowButton(NSWindowButton::MiniaturizeButton)?,
            native.standardWindowButton(NSWindowButton::ZoomButton)?,
        ])
    }

    /// The view the buttons stand in, and the title bar's container
    /// around it, which gpui sizes to the row.
    fn holders(close: &NSButton) -> (Option<Retained<NSView>>, Option<Retained<NSView>>) {
        // SAFETY: the button is the window's own, from
        // `standardWindowButton`, and each view is returned retained.
        let view = unsafe { close.superview() };
        let container = view.as_ref().and_then(|v| unsafe { v.superview() });
        (view, container)
    }

    /// Each button and their container post their moves to `place`.
    /// AppKit can make the buttons anew, and new ones are watched then.
    pub(super) fn watch(native: &NSWindow) {
        let Some(b) = buttons(native) else { return };
        let key = Retained::as_ptr(&b[0]) as usize;
        if WATCHING.with_borrow(|w| w.0 == key) {
            return;
        }
        let center = NSNotificationCenter::defaultCenter();
        WATCHING.with_borrow_mut(|w| {
            for old in w.1.drain(..) {
                unsafe { center.removeObserver(AsRef::<objc2::runtime::AnyObject>::as_ref(&*old)) };
            }
        });
        let weak = Weak::from_retained(&native.retain());
        let block = block2::RcBlock::new(move |_: NonNull<NSNotification>| {
            if let Some(native) = weak.load() {
                place(&native);
            }
        });
        let container = holders(&b[0]).1;
        let views: Vec<Retained<NSView>> = b.iter().map(|b| Retained::into_super(Retained::into_super(b.clone()))).chain(container).collect();
        let tokens = views
            .iter()
            .map(|v| {
                v.setPostsFrameChangedNotifications(true);
                unsafe { center.addObserverForName_object_queue_usingBlock(Some(NSViewFrameDidChangeNotification), Some(v), None, &block) }
            })
            .collect();
        WATCHING.set((key, tokens));
    }

    /// The buttons where the fold puts them. In full screen they are the
    /// system's, as gpui leaves them.
    pub(super) fn place(native: &NSWindow) {
        if MOVING.get() || native.styleMask().contains(NSWindowStyleMask::FullScreen) {
            return;
        }
        let Some(b) = buttons(native) else { return };
        let (Some(view), Some(container)) = holders(&b[0]) else { return };
        let (close, mini) = (b[0].frame(), b[1].frame());
        if (close.origin.y - mini.origin.y).abs() < 0.01 && mini.origin.x > close.origin.x {
            PITCH.set(Some(mini.origin.x - close.origin.x));
        }
        let Some(pitch) = PITCH.get() else { return };
        let (k, mid) = FOLD.get();
        let (k, mid) = (k as f64, mid as f64);
        let (row_x, row_y) = (super::LIGHTS_ROW.0 as f64, super::LIGHTS_ROW.1 as f64);
        let (w, h) = (close.size.width, close.size.height);
        let col_x = mid - w / 2.;
        // Each button's left and its top under the window's top.
        let at = |i: f64| (col_x + (row_x + pitch * i - col_x) * k, row_y + pitch * i * (1. - k));
        // The container holds the buttons, which take a click only
        // inside it: as tall as the column. It stays the window's width,
        // as gpui leaves it: on some systems it draws the window's rim
        // and corner at its own edge, which narrower stood as a notch
        // and a line on the rail. A click on the card under it passes
        // through to the window, as it does under gpui's own row.
        let window = native.frame().size;
        let height = at(2.).1 + h + row_y;
        MOVING.set(true);
        let mut moved = false;
        let mut frame: NSRect = container.frame();
        if (frame.size.height - height).abs() > 0.01 || (frame.origin.y - (window.height - height)).abs() > 0.01 || (frame.size.width - window.width).abs() > 0.01 || frame.origin.x.abs() > 0.01 {
            frame.size = NSSize::new(window.width, height);
            frame.origin.x = 0.;
            frame.origin.y = window.height - height;
            container.setFrame(frame);
            moved = true;
        }
        let (flipped, inside) = (view.isFlipped(), view.bounds().size.height);
        for (i, button) in b.iter().enumerate() {
            let (x, y) = at(i as f64);
            let origin = NSPoint::new(x, if flipped { y } else { inside - y - h });
            let now = button.frame().origin;
            if (now.x - origin.x).abs() > 0.01 || (now.y - origin.y).abs() > 0.01 {
                button.setFrameOrigin(origin);
                button.updateTrackingAreas();
                moved = true;
            }
        }
        if moved {
            container.updateTrackingAreas();
        }
        MOVING.set(false);
    }
}

#[cfg(target_os = "macos")]
mod languages {
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSArray, NSBundle, NSString, NSUserDefaults};

    const KEY: &str = "AppleLanguages";

    /// `AppleLanguages` as one domain of the defaults has it, alone: not
    /// the search list's, which falls back from the app's to the system's.
    fn list(domain: &str) -> Vec<String> {
        let defaults = NSUserDefaults::standardUserDefaults();
        let Some(dict) = defaults.persistentDomainForName(&NSString::from_str(domain)) else { return Vec::new() };
        let Some(value) = dict.objectForKey(&NSString::from_str(KEY)) else { return Vec::new() };
        let Ok(all) = value.downcast::<NSArray>() else { return Vec::new() };
        all.iter().filter_map(|o| o.downcast::<NSString>().ok().map(|s| s.to_string())).collect()
    }

    pub fn system() -> Vec<String> {
        list("NSGlobalDomain")
    }

    /// A bare executable has no bundle and so no domain of its own: it
    /// follows the system's.
    pub fn own() -> Option<String> {
        let id = NSBundle::mainBundle().bundleIdentifier()?;
        list(&id.to_string()).into_iter().next()
    }

    pub fn set_own(code: Option<&str>) {
        if NSBundle::mainBundle().bundleIdentifier().is_none() {
            return;
        }
        let defaults = NSUserDefaults::standardUserDefaults();
        let key = NSString::from_str(KEY);
        match code {
            Some(code) => {
                let one = NSArray::from_retained_slice(&[NSString::from_str(code)]);
                let one: &AnyObject = &one;
                unsafe { defaults.setObject_forKey(Some(one), &key) };
            }
            None => defaults.removeObjectForKey(&key),
        }
    }
}

/// The system's preferred languages, in its order, apart from any the app
/// was given of its own.
#[cfg(target_os = "macos")]
pub fn system_languages() -> Vec<String> {
    let all = languages::system();
    if all.is_empty() { sys_locale::get_locales().collect() } else { all }
}
#[cfg(not(target_os = "macos"))]
pub fn system_languages() -> Vec<String> {
    sys_locale::get_locales().collect()
}

/// The language the app was given of its own, if any: by its setting or
/// by System Settings, Language & Region, Applications, which write the
/// same key. Elsewhere the settings file keeps it.
#[cfg(target_os = "macos")]
pub fn own_language() -> Option<String> {
    languages::own()
}
#[cfg(not(target_os = "macos"))]
pub fn own_language() -> Option<String> {
    None
}

/// Give the app a language of its own (a bundle's code, `zh-Hans`), or
/// none, to follow the system's.
#[cfg(target_os = "macos")]
pub fn set_own_language(code: Option<&str>) {
    languages::set_own(code);
}
#[cfg(not(target_os = "macos"))]
pub fn set_own_language(_code: Option<&str>) {}

/// The words of `text` the system's checker does not know in `language`,
/// for the languages Harper does not read. The system's checker is the
/// Mac's (`NSSpellChecker`); Windows and Linux mark nothing. Asked on the
/// main thread and found off it, by the checker's own request in the
/// background. What a word would be is asked when its menu opens
/// (`spelling_fixes`).
#[cfg(target_os = "macos")]
pub fn spelling(text: &str, language: &str) -> impl std::future::Future<Output = Vec<onyx_core::check::Issue>> + use<> {
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSSpellChecker, NSTextCheckingOrthographyKey};
    use objc2_foundation::{NSDictionary, NSOrthography, NSRange, NSString, NSTextCheckingResult, NSTextCheckingType};
    use onyx_core::check::{Issue, Kind};
    const MOST: usize = 200;
    let (tx, rx) = futures::channel::oneshot::channel::<Vec<(usize, usize)>>();
    let tx = std::sync::Mutex::new(Some(tx));
    let handler = block2::RcBlock::new(move |_: isize, found: std::ptr::NonNull<objc2_foundation::NSArray<NSTextCheckingResult>>, _: std::ptr::NonNull<NSOrthography>, _: isize| {
        // SAFETY: the checker hands the results it found, alive for the call.
        let found = unsafe { found.as_ref() };
        let ranges = found.iter().filter(|r| r.resultType() == NSTextCheckingType::Spelling).map(|r| r.range()).map(|r| (r.location, r.length)).take(MOST).collect();
        if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
            let _ = tx.send(ranges);
        }
    });
    let string = NSString::from_str(text);
    let orthography = NSOrthography::defaultOrthographyForLanguage(&NSString::from_str(language));
    // SAFETY: the key is AppKit's, and an orthography is what it takes.
    let options: objc2::rc::Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(&[unsafe { NSTextCheckingOrthographyKey }], &[orthography.as_ref() as &AnyObject]);
    let length = text.encode_utf16().count();
    let checker = NSSpellChecker::sharedSpellChecker();
    // The language asked for and no other: left to tell the language
    // itself, the checker lets a word pass that is one in another of the
    // person's languages.
    checker.setAutomaticallyIdentifiesLanguages(false);
    checker.setLanguage(&NSString::from_str(language));
    // SAFETY: the options are the documented key with its type, and the
    // handler is called once, from any thread, which it is fit for.
    unsafe {
        checker.requestCheckingOfString_range_types_options_inSpellDocumentWithTag_completionHandler(&string, NSRange::new(0, length), NSTextCheckingType::Spelling.0, Some(&options), 0, Some(&handler));
    }
    // The checker counts UTF-16 units; the window counts bytes.
    let mut bytes = Vec::with_capacity(text.len() + 1);
    for (i, c) in text.char_indices() {
        bytes.extend(std::iter::repeat_n(i, c.len_utf16()));
    }
    bytes.push(text.len());
    async move {
        let ranges = rx.await.unwrap_or_default();
        ranges
            .into_iter()
            .filter_map(|(at, len)| Some((*bytes.get(at)?, *bytes.get(at + len)?)))
            .filter(|(start, end)| start < end)
            .map(|(start, end)| Issue { range: start..end, kind: Kind::Spelling, message: String::new(), fixes: Vec::new(), rule: String::new() })
            .collect()
    }
}

#[cfg(not(target_os = "macos"))]
pub fn spelling(_text: &str, _language: &str) -> impl std::future::Future<Output = Vec<onyx_core::check::Issue>> + use<> {
    std::future::ready(Vec::new())
}

/// What the system's checker would put in place of `word`, in
/// `language`: the five it likes best. One word, as its menu opens.
#[cfg(target_os = "macos")]
pub fn spelling_fixes(word: &str, language: &str) -> Vec<String> {
    use objc2_app_kit::NSSpellChecker;
    use objc2_foundation::{NSRange, NSString};
    let checker = NSSpellChecker::sharedSpellChecker();
    let string = NSString::from_str(word);
    let range = NSRange::new(0, word.encode_utf16().count());
    checker.guessesForWordRange_inString_language_inSpellDocumentWithTag(range, &string, Some(&NSString::from_str(language)), 0).map(|guesses| guesses.iter().take(5).map(|g| g.to_string()).collect()).unwrap_or_default()
}

#[cfg(not(target_os = "macos"))]
pub fn spelling_fixes(_word: &str, _language: &str) -> Vec<String> {
    Vec::new()
}

/// A picture on the clipboard as two things at once: `text`, which a
/// paste in Onyx reads first and takes as the same picture, and the
/// picture's own bytes, which another app takes as the picture. False
/// when its kind has no type the clipboard knows, or it could not be
/// read; the text alone is copied then.
#[cfg(target_os = "macos")]
pub fn copy_with_picture(text: &str, picture: &std::path::Path) -> bool {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::{NSData, NSString};
    let kind = match picture.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref() {
        Some("png") => "public.png",
        Some("jpg" | "jpeg") => "public.jpeg",
        Some("gif") => "com.compuserve.gif",
        Some("tif" | "tiff") => "public.tiff",
        Some("heic") => "public.heic",
        Some("webp") => "org.webmproject.webp",
        _ => return false,
    };
    let Ok(bytes) = std::fs::read(picture) else { return false };
    let Some(class) = AnyClass::get(c"NSPasteboard") else { return false };
    let data = NSData::with_bytes(&bytes);
    let (words, plain, typed) = (NSString::from_str(text), NSString::from_str("public.utf8-plain-text"), NSString::from_str(kind));
    // SAFETY: the general pasteboard's documented calls, cleared first so
    // the two types are one item.
    unsafe {
        let board: Option<Retained<AnyObject>> = msg_send![class, generalPasteboard];
        let Some(board) = board else { return false };
        let _: isize = msg_send![&*board, clearContents];
        let a: bool = msg_send![&*board, setString: &*words, forType: &*plain];
        let b: bool = msg_send![&*board, setData: &*data, forType: &*typed];
        a && b
    }
}

#[cfg(not(target_os = "macos"))]
pub fn copy_with_picture(_text: &str, _picture: &std::path::Path) -> bool {
    false
}

/// A PDF put through the system's printing, with its panel over the
/// window: the person's printers, pages and copies, and Save as PDF. The
/// paper starts as the page's. False when it could not be done.
#[cfg(target_os = "macos")]
pub fn print_pdf(window: &gpui::Window, bytes: &[u8], title: &str, page: (f32, f32)) -> bool {
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::msg_send;
    use objc2_app_kit::{NSPrintInfo, NSPrintOperation, NSPrintPanelOptions};
    use objc2_foundation::{NSData, NSSize, NSString};

    #[link(name = "PDFKit", kind = "framework")]
    unsafe extern "C" {}
    /// PDFKit's `kPDFPrintPageScaleToFit`.
    const SCALE_TO_FIT: isize = 1;

    let Some(class) = AnyClass::get(c"PDFDocument") else { return false };
    let data = NSData::with_bytes(bytes);
    // SAFETY: `PDFDocument`'s documented initializer, handed the bytes it
    // reads; it answers nil for what it cannot read.
    let document: Option<Retained<AnyObject>> = unsafe {
        let made: Allocated<AnyObject> = msg_send![class, alloc];
        msg_send![made, initWithData: &*data]
    };
    let Some(document) = document else { return false };
    let info = NSPrintInfo::sharedPrintInfo();
    // SAFETY: a copy of the shared info, so the person's other printing
    // keeps its own paper.
    let info: Retained<NSPrintInfo> = unsafe { msg_send![&*info, copy] };
    info.setPaperSize(NSSize::new(page.0 as f64, page.1 as f64));
    info.setTopMargin(0.);
    info.setBottomMargin(0.);
    info.setLeftMargin(0.);
    info.setRightMargin(0.);
    info.setHorizontallyCentered(true);
    info.setVerticallyCentered(true);
    // SAFETY: PDFKit's documented call, with its documented arguments.
    let operation: Option<Retained<NSPrintOperation>> = unsafe { msg_send![&*document, printOperationForPrintInfo: &*info, scalingMode: SCALE_TO_FIT, autoRotate: true] };
    let Some(operation) = operation else { return false };
    operation.setJobTitle(Some(&NSString::from_str(title)));
    operation.setShowsPrintPanel(true);
    operation.setShowsProgressPanel(true);
    let panel = operation.printPanel();
    panel.setOptions(NSPrintPanelOptions::ShowsCopies | NSPrintPanelOptions::ShowsPageRange | NSPrintPanelOptions::ShowsPaperSize | NSPrintPanelOptions::ShowsOrientation | NSPrintPanelOptions::ShowsScaling | NSPrintPanelOptions::ShowsPreview);
    match ns_view(window).and_then(|v| v.window()) {
        // SAFETY: no delegate and no selector: the sheet runs and closes
        // by itself, and AppKit holds the operation while it does.
        Some(native) => unsafe { operation.runOperationModalForWindow_delegate_didRunSelector_contextInfo(&native, None, None, std::ptr::null_mut()) },
        None => {
            operation.runOperation();
        }
    }
    true
}

/// Elsewhere the PDF opens in the system's viewer, which prints it.
#[cfg(not(target_os = "macos"))]
pub fn print_pdf(_window: &gpui::Window, bytes: &[u8], title: &str, _page: (f32, f32)) -> bool {
    let file = std::env::temp_dir().join(format!("{}.pdf", title.replace(['/', '\\'], "-")));
    if onyx_core::paths::write_atomic(&file, bytes).is_err() {
        return false;
    }
    open(&file.to_string_lossy());
    true
}

/// The keyboard back to the window's own view, from a system view set
/// in it (the preview's player) that a click gave it to and that is now
/// gone or out of sight; else no key binding of the window's is heard.
/// Whether a panel of the system's stands on the window as a sheet, the
/// printing panel's among them: the window under it neither scrolls nor
/// follows the pointer while it does.
#[cfg(target_os = "macos")]
pub fn sheet_over(window: &gpui::Window) -> bool {
    ns_view(window).and_then(|v| v.window()).is_some_and(|w| w.attachedSheet().is_some())
}

#[cfg(not(target_os = "macos"))]
pub fn sheet_over(_window: &gpui::Window) -> bool {
    false
}

#[cfg(target_os = "macos")]
pub fn keys_to_window(window: &gpui::Window) {
    let Some(view) = ns_view(window) else { return };
    let Some(native) = view.window() else { return };
    native.makeFirstResponder(Some(view));
}

#[cfg(not(target_os = "macos"))]
pub fn keys_to_window(_window: &gpui::Window) {}

#[cfg(target_os = "macos")]
fn ns_view(window: &gpui::Window) -> Option<&objc2_app_kit::NSView> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return None };
    unsafe { (handle.ns_view.as_ptr() as *const objc2_app_kit::NSView).as_ref() }
}

/// A picture of the app's own window, written to `path` as a PNG, for
/// looking at a state from a script (`ONYX_SHOT`). A process may capture
/// its own windows without Screen Recording access, which the terminal
/// that runs a probe usually lacks. `CGWindowListCreateImage` is looked
/// up by name: newer SDKs mark it unavailable in favor of
/// ScreenCaptureKit, which wants that access, while the function itself
/// is still there and still captures a window of the caller's own.
#[cfg(target_os = "macos")]
pub fn shoot_window(window: &gpui::Window, path: &std::path::Path) -> Result<(), String> {
    use std::ffi::{c_char, c_void, CString};
    use std::os::unix::ffi::OsStrExt;

    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    #[repr(C)]
    struct Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }
    type Capture = unsafe extern "C" fn(Rect, u32, u32, u32) -> *mut c_void;
    #[link(name = "ImageIO", kind = "framework")]
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn CFURLCreateFromFileSystemRepresentation(alloc: *const c_void, path: *const u8, len: isize, is_dir: u8) -> *mut c_void;
        fn CFStringCreateWithCString(alloc: *const c_void, text: *const c_char, encoding: u32) -> *mut c_void;
        fn CGImageDestinationCreateWithURL(url: *mut c_void, kind: *mut c_void, count: usize, options: *const c_void) -> *mut c_void;
        fn CGImageDestinationAddImage(dest: *mut c_void, image: *mut c_void, properties: *const c_void);
        fn CGImageDestinationFinalize(dest: *mut c_void) -> u8;
        fn CFRelease(obj: *mut c_void);
    }

    let handle = HasWindowHandle::window_handle(window).map_err(|e| e.to_string())?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return Err("not an AppKit window".into()) };
    let view = unsafe { (handle.ns_view.as_ptr() as *const NSView).as_ref() }.ok_or("no view")?;
    let number = view.window().ok_or("the view has no window")?.windowNumber();
    let bytes = path.as_os_str().as_bytes();
    unsafe {
        // RTLD_DEFAULT, then: no rectangle (the window's own bounds),
        // only this window, without its shadow.
        let capture = dlsym(-2isize as *mut c_void, c"CGWindowListCreateImage".as_ptr());
        if capture.is_null() {
            return Err("this system has no CGWindowListCreateImage".into());
        }
        let capture: Capture = std::mem::transmute(capture);
        let null = Rect { x: f64::INFINITY, y: f64::INFINITY, w: 0.0, h: 0.0 };
        // `ONYX_SHOT_FRAME=1` keeps what the system draws around the
        // window, its shadow and its rim.
        let framing = if std::env::var_os("ONYX_SHOT_FRAME").is_some() { 0 } else { 1 };
        let image = capture(null, 1 << 3, number as u32, framing);
        if image.is_null() {
            return Err("the window could not be captured".into());
        }
        let url = CFURLCreateFromFileSystemRepresentation(std::ptr::null(), bytes.as_ptr(), bytes.len() as isize, 0);
        let png = CString::new("public.png").unwrap();
        let kind = CFStringCreateWithCString(std::ptr::null(), png.as_ptr(), 0x0800_0100);
        let dest = CGImageDestinationCreateWithURL(url, kind, 1, std::ptr::null());
        let written = if dest.is_null() {
            false
        } else {
            CGImageDestinationAddImage(dest, image, std::ptr::null());
            let ok = CGImageDestinationFinalize(dest) != 0;
            CFRelease(dest);
            ok
        };
        CFRelease(kind);
        CFRelease(url);
        CFRelease(image);
        if written { Ok(()) } else { Err(format!("could not write {}", path.display())) }
    }
}
#[cfg(not(target_os = "macos"))]
pub fn shoot_window(_window: &gpui::Window, _path: &std::path::Path) -> Result<(), String> {
    Err("a picture of the window is only taken on macOS".into())
}

/// The window kept over every other app's, for a probe.
#[cfg(target_os = "macos")]
pub fn keep_on_top(window: &gpui::Window) {
    if let Some(native) = ns_view(window).and_then(|v| v.window()) {
        native.setLevel(objc2_app_kit::NSFloatingWindowLevel);
    }
}
#[cfg(not(target_os = "macos"))]
pub fn keep_on_top(_window: &gpui::Window) {}

/// The account's first name, else the login name, capitalized: whom the
/// vault's page greets. The same crate asks every system.
pub fn user_first_name() -> String {
    // An iPhone tells an app no name: it is the one a Mac's Onyx shared
    // through iCloud (`share_name`), else none until the person gives one.
    #[cfg(target_os = "ios")]
    {
        SHARED_NAME.get().cloned().unwrap_or_default()
    }
    #[cfg(not(target_os = "ios"))]
    {
        let full = whoami::fallible::realname().unwrap_or_default();
        let name = full.split_whitespace().next().map(str::to_string).filter(|s| !s.is_empty()).or_else(|| whoami::fallible::username().ok()).unwrap_or_default();
        let mut chars = name.chars();
        match chars.next() {
            Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            None => String::new(),
        }
    }
}

// -- the menu bar, the shortcut, login ------------------------------------------

/// A line of the menu bar item's menu: an item (what it is answered by,
/// its words, the picture before them, and its key equivalent as AppKit
/// takes it, the character and the modifier flags), or a line between
/// groups.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum ItemLine {
    Item { key: String, title: String, icon: &'static [u8], keys: (String, u64) },
    Gap,
}

/// What the menu bar item sent: a click that opens or closes the note,
/// one that shows the menu, or an item of the menu by its key.
#[derive(Debug, Clone)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum ItemPick {
    Note,
    Menu,
    Line(String),
}

/// The item in the menu bar, shown or not, and what is chosen from it.
/// Elsewhere there is no item: Windows and Linux are not published, and
/// their tray is left for when they are.
#[cfg(target_os = "macos")]
pub fn install_menu_bar(shown: bool) -> Option<futures::channel::mpsc::UnboundedReceiver<ItemPick>> {
    status_item::install(shown)
}
#[cfg(not(target_os = "macos"))]
pub fn install_menu_bar(_shown: bool) -> Option<futures::channel::mpsc::UnboundedReceiver<ItemPick>> {
    None
}

#[cfg(target_os = "macos")]
pub fn set_menu_bar_shown(shown: bool) {
    status_item::set_shown(shown);
}
#[cfg(not(target_os = "macos"))]
pub fn set_menu_bar_shown(_shown: bool) {}

/// The item's badge filled while the note is open.
#[cfg(target_os = "macos")]
pub fn set_menu_bar_open(open: bool) {
    status_item::set_open(open);
}
#[cfg(not(target_os = "macos"))]
pub fn set_menu_bar_open(_open: bool) {}

/// The menu's lines from now on, built when it is next shown.
#[cfg(target_os = "macos")]
pub fn set_menu_bar_lines(lines: Vec<ItemLine>) {
    status_item::set_lines(lines);
}
#[cfg(not(target_os = "macos"))]
pub fn set_menu_bar_lines(_lines: Vec<ItemLine>) {}

/// Where the item stands, from the top left of the first screen: x, y,
/// width, height. None while it is hidden, off every screen, or nowhere.
#[cfg(target_os = "macos")]
pub fn menu_bar_frame() -> Option<(f32, f32, f32, f32)> {
    status_item::frame()
}
#[cfg(not(target_os = "macos"))]
pub fn menu_bar_frame() -> Option<(f32, f32, f32, f32)> {
    None
}

/// The item put back in sight when a menu bar manager left it off the
/// screen.
#[cfg(target_os = "macos")]
pub fn heal_menu_bar() {
    status_item::heal();
}
#[cfg(not(target_os = "macos"))]
pub fn heal_menu_bar() {}

/// The handler for the system's hot keys, installed once, and the presses
/// it hears, each the number its shortcut was held by.
#[cfg(target_os = "macos")]
pub fn install_hot_keys() -> Option<futures::channel::mpsc::UnboundedReceiver<u32>> {
    hotkeys::install()
}
#[cfg(not(target_os = "macos"))]
pub fn install_hot_keys() -> Option<futures::channel::mpsc::UnboundedReceiver<u32>> {
    None
}

/// The hot keys held from now on, the old ones given back first; the
/// numbers of those the system would not take.
#[cfg(target_os = "macos")]
pub fn set_hot_keys(keys: &[(u32, onyx_core::shortcut::Shortcut)]) -> Vec<u32> {
    hotkeys::set(keys)
}
#[cfg(not(target_os = "macos"))]
pub fn set_hot_keys(keys: &[(u32, onyx_core::shortcut::Shortcut)]) -> Vec<u32> {
    keys.iter().map(|(n, _)| *n).collect()
}

/// The watch on keys pressed in Onyx's windows while a shortcut is being
/// recorded, installed once: each as its flags and key code.
#[cfg(target_os = "macos")]
pub fn install_key_recorder() -> Option<futures::channel::mpsc::UnboundedReceiver<(u64, u16)>> {
    Some(keys::install())
}
#[cfg(not(target_os = "macos"))]
pub fn install_key_recorder() -> Option<futures::channel::mpsc::UnboundedReceiver<(u64, u16)>> {
    None
}

/// While on, a key pressed in Onyx's windows goes to the recorder alone.
#[cfg(target_os = "macos")]
pub fn record_keys(on: bool) {
    keys::recording(on);
}
#[cfg(not(target_os = "macos"))]
pub fn record_keys(_on: bool) {}

/// The menu bar note's window over every app's, at the floating level:
/// the panel gpui makes sits at the pop-up menu's, and an input method's
/// list of candidates, which sits at that level or under it, stood behind
/// it (MiniNotes' `StatusBarController`). It is made borderless: gpui's
/// panel is titled, and the system drew a titled window's rim and backing
/// round the whole of it, the arrow's strip too, with a square shadow.
/// Borderless, its shape and its shadow are what it draws, the shadow in
/// room the window keeps round the card.
#[cfg(target_os = "macos")]
pub fn float_note_window(window: &gpui::Window) {
    use objc2_app_kit::NSWindowStyleMask;
    let Some(view) = ns_view(window) else { return };
    if let Some(native) = view.window() {
        // Unseen first, before the new style mask rebuilds its frame.
        native.setAlphaValue(0.);
        native.setStyleMask(NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel);
        native.setLevel(objc2_app_kit::NSFloatingWindowLevel);
        native.setOpaque(false);
        // Its shadow is its own (`menubar::SHADE`): the system's for a
        // window with the keyboard is deep, a popover's is light.
        native.setHasShadow(false);
        // Faded in from its first frame by its owner (`set_window_alpha`);
        // the system's own motion for a window coming would run over that.
        native.setAnimationBehavior(objc2_app_kit::NSWindowAnimationBehavior::None);
        // A new style mask rebuilds the window's frame and leaves the
        // window itself answering keys: gpui's view is made the one again,
        // or no key reaches the note while a click still does.
        native.makeFirstResponder(Some(view));
    }
}
#[cfg(not(target_os = "macos"))]
pub fn float_note_window(_window: &gpui::Window) {}

/// How much of a window shows, 0 to 1, as the system composes it: the
/// whole window, its shadow with it, with nothing drawn again.
#[cfg(target_os = "macos")]
pub fn set_window_alpha(window: &gpui::Window, alpha: f32) {
    if let Some(native) = ns_view(window).and_then(|v| v.window()) {
        native.setAlphaValue(alpha.clamp(0., 1.) as f64);
    }
}
#[cfg(not(target_os = "macos"))]
pub fn set_window_alpha(_window: &gpui::Window, _alpha: f32) {}

/// Whether Onyx opens when the person logs in, as System Settings' Login
/// Items has it: the system keeps it, and Onyx never writes it down.
/// Elsewhere no.
#[cfg(target_os = "macos")]
pub fn launches_at_login() -> bool {
    use objc2_service_management::{SMAppService, SMAppServiceStatus};
    unsafe { SMAppService::mainAppService().status() == SMAppServiceStatus::Enabled }
}
#[cfg(not(target_os = "macos"))]
pub fn launches_at_login() -> bool {
    false
}

#[cfg(target_os = "macos")]
pub fn set_launch_at_login(on: bool) -> Result<(), String> {
    use objc2_service_management::SMAppService;
    let service = unsafe { SMAppService::mainAppService() };
    let done = unsafe { if on { service.registerAndReturnError() } else { service.unregisterAndReturnError() } };
    done.map_err(|e| e.localizedDescription().to_string())
}
#[cfg(not(target_os = "macos"))]
pub fn set_launch_at_login(_on: bool) -> Result<(), String> {
    Err("not on this system".into())
}

/// Whether this launch is the system's at login: the event that opened
/// the app says so (`keyAEPropData` is `lgit`). Asked as the app finishes
/// launching, while that event is still the current one. Such a launch
/// starts in the menu bar, with no window.
#[cfg(target_os = "macos")]
pub fn launched_at_login() -> bool {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    const fn four(code: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*code)
    }
    unsafe {
        let manager: *mut AnyObject = msg_send![class!(NSAppleEventManager), sharedAppleEventManager];
        if manager.is_null() {
            return false;
        }
        let event: *mut AnyObject = msg_send![manager, currentAppleEvent];
        if event.is_null() {
            return false;
        }
        let prop: *mut AnyObject = msg_send![event, paramDescriptorForKeyword: four(b"prdt")];
        if prop.is_null() {
            return false;
        }
        let value: u32 = msg_send![prop, enumCodeValue];
        value == four(b"lgit")
    }
}
#[cfg(not(target_os = "macos"))]
pub fn launched_at_login() -> bool {
    false
}
