//! A picture of a note over the whole window, as Craft's viewer has it:
//! it grows from where it stands on the canvas to as large as the window
//! lets it, on a dark ground, and goes back there when it closes. ← and →
//! step through the note's other pictures; Esc, Space or a click on the
//! ground closes it. `docs/editor.md`, Pictures.

use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{h_flex, ActiveTheme as _, Icon, Sizable as _};

use crate::editor::{Seen, Viewing};
use crate::shell::Shell;
use crate::widgets::{swallow_click, SLIDE};

actions!(onyx_viewer, [ViewerPrev, ViewerNext, ViewerClose]);

pub const VIEWER_CONTEXT: &str = "Viewer";

/// Clear round the picture, and at the top for the traffic lights.
const CLEAR: f32 = 48.;
const CLEAR_TOP: f32 = 56.;

pub(crate) struct Viewer {
    pictures: Vec<Seen>,
    at: usize,
    /// The picture it opened on, and where that one stands on the canvas.
    first: usize,
    from: Bounds<Pixels>,
    opened: Instant,
    closing: Option<Instant>,
    /// When ← or → last brought a picture, which fades in.
    turned: Option<Instant>,
}

fn eased(since: Instant) -> f32 {
    let t = (since.elapsed().as_secs_f32() / SLIDE.as_secs_f32()).min(1.);
    ease_out_quint()(t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

impl Shell {
    /// The viewer over the window. It takes the keyboard as it is drawn.
    pub(crate) fn open_viewer(&mut self, viewing: &Viewing, cx: &mut Context<Self>) {
        let pictures = viewing.pictures.iter().map(|s| Seen { path: s.path.clone(), size: s.size, drawn: s.drawn.clone() }).collect();
        self.viewer = Some(Viewer { pictures, at: viewing.at, first: viewing.at, from: viewing.from, opened: Instant::now(), closing: None, turned: None });
        cx.notify();
    }

    /// Back to the canvas over a moment, then gone.
    pub(crate) fn close_viewer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(v) = self.viewer.as_mut() else { return };
        if v.closing.is_some() {
            return;
        }
        // Closed while it still grows, it goes back from where it is.
        let shown = eased(v.opened);
        v.closing = Some(Instant::now() - SLIDE.mul_f32(1. - shown));
        self.focus_editor_or_shell(window, cx);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SLIDE + Duration::from_millis(16)).await;
            let _ = this.update(cx, |s, cx| {
                if s.viewer.as_ref().is_some_and(|v| v.closing.is_some()) {
                    s.viewer = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn step_viewer(&mut self, by: isize, cx: &mut Context<Self>) {
        let Some(v) = self.viewer.as_mut().filter(|v| v.closing.is_none()) else { return };
        let to = v.at as isize + by;
        if to < 0 || to as usize >= v.pictures.len() {
            return;
        }
        v.at = to as usize;
        v.turned = Some(Instant::now());
        cx.notify();
    }

    pub(crate) fn render_viewer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let v = self.viewer.as_ref()?;
        if v.closing.is_none() && !self.viewer_focus.is_focused(window) {
            window.focus(&self.viewer_focus, cx);
        }
        let theme = cx.theme().clone();
        let open = eased(v.opened);
        let shown = match v.closing {
            Some(at) => (1. - eased(at)).min(open),
            None => open,
        };
        let turning = v.turned.map_or(1., eased);
        if shown < 1. && v.closing.is_some() || open < 1. || turning < 1. {
            window.request_animation_frame();
        }
        let seen = v.pictures.get(v.at)?;
        // As large as the window lets it, and never past its own pixels.
        let view = window.viewport_size();
        let (vw, vh) = (f32::from(view.width), f32::from(view.height));
        let (pw, ph) = (seen.size.0.max(1) as f32, seen.size.1.max(1) as f32);
        let k = ((vw - CLEAR * 2.) / pw).min((vh - CLEAR_TOP - CLEAR) / ph).min(1.).max(0.01);
        let (tw, th) = (pw * k, ph * k);
        let (tx, ty) = ((vw - tw) / 2., CLEAR_TOP + (vh - CLEAR_TOP - CLEAR - th) / 2.);
        // The picture it opened on grows from the canvas and goes back
        // there; one stepped to fades in and out where it stands.
        let from_canvas = v.at == v.first;
        let (x, y, w, h, alpha) = if from_canvas {
            let f = v.from;
            let (fx, fy, fw, fh) = (f32::from(f.origin.x), f32::from(f.origin.y), f32::from(f.size.width), f32::from(f.size.height));
            (lerp(fx, tx, shown), lerp(fy, ty, shown), lerp(fw, tw, shown), lerp(fh, th, shown), 1.)
        } else {
            (tx, ty, tw, th, shown.min(turning))
        };
        let source: ImageSource = match &seen.drawn {
            Some(image) => ImageSource::Render(image.clone()),
            None => seen.path.clone().into(),
        };
        let count = v.pictures.len();
        let at = v.at;
        let white = gpui::white();
        let round = |id: &'static str, icon: &'static str| {
            div()
                .id(id)
                .size(px(34.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(white.opacity(0.12))
                .cursor_pointer()
                .hover(|s| s.bg(white.opacity(0.22)))
                .child(Icon::default().path(icon).with_size(px(16.)).text_color(white))
        };
        let controls = shown * if v.closing.is_some() { 1. } else { open };
        Some(
            div()
                .id("viewer")
                .key_context(VIEWER_CONTEXT)
                .track_focus(&self.viewer_focus)
                .on_action(cx.listener(|this, _: &ViewerClose, window, cx| this.close_viewer(window, cx)))
                .on_action(cx.listener(|this, _: &crate::shell::Escape, window, cx| this.close_viewer(window, cx)))
                .on_action(cx.listener(|this, _: &ViewerPrev, _, cx| this.step_viewer(-1, cx)))
                .on_action(cx.listener(|this, _: &ViewerNext, _, cx| this.step_viewer(1, cx)))
                .absolute()
                .inset_0()
                .occlude()
                .bg(black().opacity(0.86 * shown))
                .on_click(cx.listener(|this, _, window, cx| this.close_viewer(window, cx)))
                .child(
                    img(source)
                        .absolute()
                        .left(px(x))
                        .top(px(y))
                        .w(px(w))
                        .h(px(h))
                        .rounded(px(crate::editor::PICTURE_ROUND))
                        .opacity(alpha)
                        .object_fit(ObjectFit::Fill),
                )
                .child(
                    div().absolute().top(px(14.)).right(px(16.)).opacity(controls).child(round("viewer-close", "icons/close.svg").on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.close_viewer(window, cx);
                    }))),
                )
                .when(count > 1, |d| {
                    d.child(
                        h_flex()
                            .absolute()
                            .bottom(px(14.))
                            .left_0()
                            .right_0()
                            .justify_center()
                            .gap(px(14.))
                            .items_center()
                            .opacity(controls)
                            .child(round("viewer-prev", "icons/chevron-left.svg").when(at == 0, |d| d.opacity(0.35)).on_click(cx.listener(|this, _, window, cx| {
                                swallow_click(window, cx);
                                this.step_viewer(-1, cx);
                            })))
                            .child(div().min_w(px(56.)).text_center().text_size(px(12.5)).text_color(white.opacity(0.8)).child(format!("{} / {count}", at + 1)))
                            .child(round("viewer-next", "icons/chevron-right.svg").when(at + 1 >= count, |d| d.opacity(0.35)).on_click(cx.listener(|this, _, window, cx| {
                                swallow_click(window, cx);
                                this.step_viewer(1, cx);
                            }))),
                    )
                })
                .text_color(theme.foreground)
                .into_any_element(),
        )
    }
}
