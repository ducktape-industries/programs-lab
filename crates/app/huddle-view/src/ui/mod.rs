//! The desktop's tree: what Huddle draws under a pointer. The device check
//! (the picture with its toggles, the settings column, a call ringing you
//! over it), the room (the stage of the people in it and its bar, or below
//! 480 by 320 one tile with its controls over it), the menus, the state
//! cards a refused or missing device shows, and the screen of a build
//! without camera and microphone. The call's own parts are `call.rs`'s.
//! Pieces sit first in their parent, so the cards and chips after them draw
//! over the picture. The host keys a piece by its path: the root always
//! goes through the one menu overlay, and both room layouts hold the pieces
//! under the same room, stage and tile (a person's tile by their number),
//! so a menu, a float or a new speaker moving tiles opens no device and
//! subscribes to no track again.
use std::ops::Range;
use std::rc::Rc;

use view::prelude::*;
use view::{
    Anchor, AnchoredPositionMode, AnyElement, Bounds, Div, Edges, FontWeight, Point, Stateful,
    accesskit,
};

use crate::look::{
    self, ALERT, CHECK, HEADPHONES, LEAVE, MIC, MIC_OFF, PLAY, SPEAKER, SPEAKING, STOP, VIDEO,
    VIDEO_OFF, black, glass, line_glyph, muted_mark, trouble_glyph, white,
};
use crate::people::{self, PEOPLE};
use crate::words::{self, Device};
use crate::{Huddle, Kind, Menu, RECORD_SECONDS, Row, Screen, Which, trouble};

mod call;
mod door;
/// The phone's tree, which the view's root picks under a finger. It reads
/// nothing of this one.
pub(crate) mod phone;

/// The check's settings column, and the menus' width.
const COLUMN: f32 = 272.;

/// Below this the room is one tile with its controls over it.
const COMPACT: (f32, f32) = (480., 320.);

/// The room's bar, under the stage.
const BAR: Pixels = px(48.);

/// A field's label over its control: one line of CAPTION.
const FIELD_LABEL: Pixels = px(15.);

/// The accesskit popup kind a picker and a caret announce.
const MENU_POPUP: accesskit::HasPopup = accesskit::HasPopup::Menu;

/// What takes the keys: the room on Join, Join on Leave, the open menu.
const ROOM: &str = "huddle-room";
const JOIN: &str = "huddle-join";
const MENU: &str = "huddle-menu";

/// A menu longer than this many lines scrolls.
const MENU_LINES: usize = 8;

/// What the desktop's tree keeps between frames.
#[derive(Default)]
pub(crate) struct Desk {
    /// Where layout put each menu's opener, in window pixels: a menu hangs
    /// under its picker, or over the room's caret.
    anchors: Vec<(Which, Bounds<Pixels>)>,
    /// The open menu's list, scrolled to its active row.
    menu_scroll: UniformListScrollHandle,
}

impl Desk {
    /// The menu's opener's bounds, as layout last put it.
    fn anchor(&self, which: Which) -> Option<Bounds<Pixels>> {
        self.anchors
            .iter()
            .find(|(at, _)| *at == which)
            .map(|(_, bounds)| *bounds)
    }

    fn set_anchor(&mut self, which: Which, bounds: Bounds<Pixels>) {
        self.anchors.retain(|(at, _)| *at != which);
        self.anchors.push((which, bounds));
    }
}

/// The room is one tile with its controls over it: below 480 by 320.
fn compact(view: &Huddle) -> bool {
    let (width, height) = view.viewport;
    width < COMPACT.0 || height < COMPACT.1
}

/// Into the room, the keys in it.
fn join(view: &mut Huddle, window: &mut Window, cx: &mut Context<Huddle>) {
    view.join(cx);
    window.focus(ROOM);
}

/// Out of the room, the keys on Join.
fn leave(view: &mut Huddle, window: &mut Window, cx: &mut Context<Huddle>) {
    view.leave(cx);
    window.focus(JOIN);
}

/// The menu of `which` opened by the control `opener`, the keys in it.
fn open(
    view: &mut Huddle,
    which: Which,
    opener: &'static str,
    window: &mut Window,
    cx: &mut Context<Huddle>,
) {
    view.open_menu(which, opener, cx);
    if let Some(menu) = view.menu {
        // centred, so the pick shows with its neighbours; the arrows keep
        // Nearest
        reveal(view, which, menu.cursor, ScrollStrategy::Center);
    }
    window.focus(MENU);
}

/// A menu closed (a pick, Escape, a press outside): the keys back on the
/// control that opened it.
fn closed(menu: Option<Menu>, window: &mut Window) {
    if let Some(menu) = menu {
        window.focus(menu.opener);
    }
}

// the desktop's own glyphs, by the Lucide names `design::icon` draws (the
// ones the phone draws too are `look`'s)
const CARET: &str = "chevron-down";
const MIRROR: &str = "flip-horizontal-2";
const PIN: &str = "pin";
const PIN_OFF: &str = "pin-off";
const BACK: &str = "picture-in-picture-2";
const CLOSE: &str = "x";

fn f(pixels: Pixels) -> f32 {
    pixels.into()
}

pub(crate) fn render(view: &mut Huddle, cx: &mut Context<Huddle>) -> AnyElement {
    let theme = *cx.global::<Theme>();
    let screen = match (view.no_support(), view.screen, &view.door) {
        (true, ..) => unsupported(&theme).into_any_element(),
        (false, Screen::Check, Some(door)) => {
            door::screen(view, door, cx, &theme).into_any_element()
        }
        (false, Screen::Check, None) => check(view, cx, &theme).into_any_element(),
        (false, Screen::Room, _) => room(view, cx, &theme).into_any_element(),
    };
    let base = div()
        .id("huddle")
        .size_full()
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_size(design::text::BODY)
        // the boards' leading: a label sits 15 px tall over its control
        .line_height(gpui::relative(1.3))
        .child(screen);
    let open = view.menu.filter(|_| !view.no_support());
    let (label, floating) = match open {
        Some(open) => (words::menu(open.which), Some(menu(view, open, cx, &theme))),
        None => ("", None),
    };
    let mut root = modal_overlay("huddle-menu-overlay", label, base, floating);
    if open.is_some() {
        root = root.on_dismiss(
            cx.listener(|view, _: &(), window, cx| closed(view.close_menu(cx), window)),
        );
    }
    root.into_any_element()
}

// ---------- the device check ----------

fn check(view: &mut Huddle, cx: &mut Context<Huddle>, theme: &Theme) -> Stateful<Div> {
    let (block, column) = (f(design::space::BLOCK), COLUMN);
    // the picture's own shape, 16:9, as wide as the column leaves; a short
    // wide window gives its height first
    let (width, height) = view.viewport;
    let mut w = (width - 3. * block - column).max(0.);
    let mut h = w * 9. / 16.;
    let room = (height - 2. * block).max(0.);
    if h > room {
        h = room;
        w = h * 16. / 9.;
    }
    div()
        .id("huddle-check")
        .size_full()
        .flex()
        .items_start()
        .p(design::space::BLOCK)
        .gap(design::space::BLOCK)
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .child(preview(view, w, h, cx, theme)),
        )
        .child(settings(view, cx, theme))
}

/// Your picture, the mirror, mic and camera toggles over it, the chip that
/// says it is you, and a card when the camera is off or in trouble.
fn preview(
    view: &Huddle,
    w: f32,
    h: f32,
    cx: &mut Context<Huddle>,
    theme: &Theme,
) -> Stateful<Div> {
    let off = view.camera_off;
    let troubled = view.troubled(Device::Camera);
    let mut preview = div()
        .id("huddle-preview")
        .relative()
        .w(px(w))
        .h(px(h))
        .flex_none()
        .overflow_hidden()
        .bg(match off || troubled {
            true => theme.surface_raised,
            false => black(),
        });
    if view.camera_placed() {
        preview = preview.child(camera_piece(view, ObjectFit::Contain, cx).size_full());
    }
    if off {
        preview = preview
            .child(look::face(px(72.), theme).child(design::quiet(words::CAMERA_IS_OFF, theme)));
    }
    let mirrored = !view.unmirrored;
    preview
        // nothing to mirror with the camera off or in trouble
        .child(mirror_toggle(mirrored, !off && !troubled, cx))
        .child(
            div()
                .absolute()
                .bottom(design::space::SM)
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .gap(design::space::SM)
                .child(mic_toggle(view.muted, theme.danger, cx))
                .child(camera_toggle(off, theme.danger, cx)),
        )
        .children(troubled.then(|| state_card(view, Device::Camera, cx, theme)))
        .child(chip(view, off, theme))
}

fn camera_piece(view: &Huddle, fit: ObjectFit, cx: &mut Context<Huddle>) -> view::Camera {
    let piece = camera(view.camera_key())
        .mirror(!view.unmirrored)
        .object_fit(fit)
        .on_state(cx.listener(|view: &mut Huddle, state: &MediaState, _, cx| {
            view.heard_camera(state, cx);
            cx.notify();
        }));
    let piece = match view.own_track(Device::Camera) {
        Some(track) => piece.publish(track),
        None => piece,
    };
    match &view.camera {
        Some(id) => piece.device(id.clone()),
        None => piece,
    }
}

fn mic_piece(view: &Huddle, cx: &mut Context<Huddle>) -> view::Microphone {
    let piece = microphone(view.mic_key())
        .monitor(view.monitor && view.screen == Screen::Check)
        .track(&view.recorder)
        .on_state(cx.listener(|view: &mut Huddle, state: &MediaState, _, cx| {
            view.heard_mic(state);
            view.heard_own_track(&state.track, cx);
            cx.notify();
        }));
    let piece = match view.own_track(Device::Microphone) {
        Some(track) => piece.publish(track).muted(view.muted),
        None => piece,
    };
    let piece = match &view.microphone {
        Some(id) => piece.device(id.clone()),
        None => piece,
    };
    match &view.speaker {
        Some(id) => piece.output(id.clone()),
        None => piece,
    }
}

/// A 32 px glass toggle over the picture: danger-filled while off.
fn glass_toggle(
    id: &'static str,
    icon: &'static str,
    name: &'static str,
    off: bool,
    danger: Hsla,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(32.))
        .flex()
        .items_center()
        .justify_center()
        .text_color(white())
        .bg(glass())
        .when(off, |button| button.bg(danger))
        .cursor_pointer()
        .role(Role::Button)
        .aria_label(name)
        .aria_toggled(off.into())
        .focusable()
        .on_click(click)
        .child(design::icon(icon, px(16.)))
}

fn mic_toggle(muted: bool, danger: Hsla, cx: &mut Context<Huddle>) -> Stateful<Div> {
    glass_toggle(
        "huddle-mic",
        if muted { MIC_OFF } else { MIC },
        words::mute_microphone(muted),
        muted,
        danger,
        cx.listener(|view: &mut Huddle, _: &ClickEvent, _, cx| view.toggle_mute(cx)),
    )
}

fn camera_toggle(off: bool, danger: Hsla, cx: &mut Context<Huddle>) -> Stateful<Div> {
    glass_toggle(
        "huddle-camera",
        if off { VIDEO_OFF } else { VIDEO },
        words::camera_toggle(off),
        off,
        danger,
        cx.listener(|view: &mut Huddle, _: &ClickEvent, _, cx| view.toggle_camera(cx)),
    )
}

/// The mirror toggle, top right: white while mirroring, glass when not,
/// faded with the camera off.
fn mirror_toggle(mirrored: bool, enabled: bool, cx: &mut Context<Huddle>) -> Stateful<Div> {
    let toggle = div()
        .id("huddle-mirror")
        .absolute()
        .top(design::space::SM)
        .right(design::space::SM)
        .size(px(28.))
        .flex()
        .items_center()
        .justify_center()
        .map(|toggle| match mirrored && enabled {
            true => toggle.bg(white()).text_color(rgb(0x111111)),
            false => toggle.bg(glass()).text_color(white()),
        })
        .role(Role::Button)
        .aria_label(words::MIRROR)
        .aria_toggled(mirrored.into())
        .child(design::icon(MIRROR, px(14.)));
    match enabled {
        true => toggle.cursor_pointer().focusable().on_click(cx.listener(
            |view: &mut Huddle, _: &ClickEvent, _, cx| {
                view.unmirrored = !view.unmirrored;
                cx.notify();
            },
        )),
        false => toggle.opacity(0.5).aria_disabled(true),
    }
}

/// "You" in the picture's corner, "· mirrored" while it is; the camera's
/// opening said here once it takes a while.
fn chip(view: &Huddle, off: bool, theme: &Theme) -> Div {
    let opening = view.camera_slow();
    let mirrored = !view.unmirrored && !off && !trouble(&view.camera_state);
    div()
        .absolute()
        .left(design::space::SM)
        .bottom(design::space::SM)
        .h(px(22.))
        .px(design::space::XS)
        .flex()
        .items_center()
        .gap(design::space::XS)
        .text_size(design::text::CAPTION)
        .map(|chip| match off {
            // on the raised ground of a camera that is off: the window's
            true => chip
                .bg(theme.background)
                .text_color(theme.foreground)
                .border_1()
                .border_color(theme.border),
            false => chip.bg(glass()).text_color(white()),
        })
        .child(match opening {
            true => words::OPENING_CAMERA,
            false => words::YOU,
        })
        .when(mirrored && !opening, |chip| {
            chip.child(
                div()
                    .font_family(design::fonts::FAMILY_MONO)
                    .opacity(0.7)
                    .child(words::MIRRORED),
            )
        })
}

/// The settings column: the three devices, the meter, the tests, Join.
fn settings(view: &Huddle, cx: &mut Context<Huddle>, theme: &Theme) -> Stateful<Div> {
    let theme = *theme;
    div()
        .id("huddle-settings")
        .w(px(COLUMN))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(14.))
        .children(call::rings(view, cx, &theme))
        .child(field(
            words::kind(Kind::Camera),
            picker(view, Kind::Camera, VIDEO, cx, &theme),
            &theme,
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(design::space::XS)
                .child(field(
                    words::kind(Kind::Microphone),
                    picker(view, Kind::Microphone, MIC, cx, &theme),
                    &theme,
                ))
                .child(meter(view, cx, &theme)),
        )
        .child(
            field(
                words::kind(Kind::Speaker),
                div()
                    .flex()
                    // Test as tall as the picker beside it
                    .items_stretch()
                    .gap(design::space::XS)
                    .child(picker(view, Kind::Speaker, SPEAKER, cx, &theme).flex_1())
                    .child(speaker_test(view, cx, &theme)),
                &theme,
            )
            .children(view.speaker_fault.as_ref().map(|why| {
                div()
                    .id("huddle-speaker-fault")
                    .text_size(design::text::CAPTION)
                    .text_color(theme.muted)
                    .child(words::test_failed(why))
            })),
        )
        .child(div().h(px(1.)).flex_none().bg(theme.border))
        .child(record(view, cx, &theme))
        .child(monitor(view, cx, &theme))
        .child(div().flex_1())
        .child(join_button(cx, &theme))
}

fn field(label: &'static str, control: impl IntoElement, theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(design::space::XXS)
        .child(
            div()
                .text_size(design::text::CAPTION)
                .text_color(theme.muted)
                .child(label),
        )
        .child(control)
}

/// One device's picker: its glyph, the device, a caret; it opens the
/// device's menu under itself (press, Enter, Space or ↓).
fn picker(
    view: &Huddle,
    kind: Kind,
    icon: &'static str,
    cx: &mut Context<Huddle>,
    theme: &Theme,
) -> view::Sensor {
    let theme = *theme;
    let which = Which::Kind(kind);
    let id = match kind {
        Kind::Camera => "huddle-pick-camera",
        Kind::Microphone => "huddle-pick-microphone",
        Kind::Speaker => "huddle-pick-speaker",
        Kind::Person => "huddle-pick-person",
    };
    let expanded = view.menu.is_some_and(|menu| menu.which == which);
    let name = view.shown_device(kind);
    let button = div()
        .id(id)
        .h(design::size::CONTROL)
        .w_full()
        .px(design::space::SM)
        .flex()
        .items_center()
        .gap(design::space::SM)
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.background)
        .text_size(design::text::SECONDARY)
        .cursor_pointer()
        .hover(move |style| style.bg(theme.surface))
        .role(Role::Button)
        .aria_has_popup(MENU_POPUP)
        .aria_expanded(expanded)
        .aria_label(format!("{}: {name}", words::kind(kind)))
        .focusable()
        .on_click(
            cx.listener(move |view: &mut Huddle, _: &ClickEvent, window, cx| {
                open(view, which, id, window, cx)
            }),
        )
        .on_key_down(
            cx.listener(move |view: &mut Huddle, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "down" && !event.keystroke.modifiers.modified() {
                    open(view, which, id, window, cx);
                }
            }),
        )
        .child(design::icon(icon, px(14.)))
        .child(div().flex_1().min_w(px(0.)).truncate().child(name))
        .child(design::icon(CARET, px(12.)).text_color(theme.muted));
    anchor(which, id, button, cx).w_full()
}

/// `control` in a sensor named after the menu's opener `id`: where layout
/// puts it is where the menu of `which` hangs (a picker, or the room's
/// split around its caret).
fn anchor(
    which: Which,
    id: &str,
    control: impl IntoElement,
    cx: &mut Context<Huddle>,
) -> view::Sensor {
    sensor(format!("{id}-at"), control).on_bounds(cx.listener(
        move |view: &mut Huddle, bounds: &Bounds<Pixels>, _, cx| {
            view.desk.set_anchor(which, *bounds);
            if view.menu.is_some_and(|menu| menu.which == which) {
                cx.notify();
            }
        },
    ))
}

/// The microphone's level, drawn by the host in the bar's colour; with
/// the microphone muted or in trouble, its line instead.
fn meter(view: &Huddle, cx: &mut Context<Huddle>, theme: &Theme) -> AnyElement {
    if view.troubled(Device::Microphone) {
        return mic_trouble(view, cx, theme).into_any_element();
    }
    div()
        .id("huddle-meter")
        .h(px(6.))
        .w_full()
        .bg(theme.surface_raised)
        .role(Role::Meter)
        .aria_label(words::MICROPHONE_LEVEL)
        .when(view.mic_placed(), |meter| {
            meter.child(
                mic_piece(view, cx)
                    .size_full()
                    .bg(theme.surface_raised)
                    .text_color(theme.foreground),
            )
        })
        .into_any_element()
}

/// A microphone that is refused, missing, busy or gone: one line and
/// what fixes it, where the meter was. The line carries the fix a refusal
/// needs, and why the microphone went away when the host says.
fn mic_trouble(view: &Huddle, cx: &mut Context<Huddle>, theme: &Theme) -> Stateful<Div> {
    let state = &view.mic_state;
    let (title, text) = words::trouble(Device::Microphone, state);
    let line = match state.status {
        MediaStatus::Denied | MediaStatus::Failed => format!("{title}. {text}"),
        MediaStatus::Gone if !state.detail.is_empty() => format!("{title}. {text}"),
        _ => title.to_owned(),
    };
    div()
        .id("huddle-mic-trouble")
        .flex()
        .flex_col()
        .gap(design::space::XS)
        .child(
            div()
                .flex()
                .items_start()
                .gap(design::space::XS)
                .text_size(design::text::CAPTION)
                .text_color(theme.muted)
                .child(design::icon(ALERT, px(12.)).mt(px(1.)))
                .child(div().flex_1().min_w(px(0.)).child(line)),
        )
        .child(actions(view, Device::Microphone, cx, theme))
}

/// Test: the host's chime on the picked speaker; muted or with the
/// microphone in trouble too (an output, apart from the microphone), not
/// over a recording's playback.
fn speaker_test(view: &Huddle, cx: &mut Context<Huddle>, theme: &Theme) -> impl IntoElement {
    let label = match view.chiming {
        true => words::PLAYING,
        false => words::TEST,
    };
    let chime = cx.listener(|view: &mut Huddle, _: &ClickEvent, _, cx| view.chime(cx));
    design::button("huddle-speaker-test", label, &design::POINTER, theme, chime)
        .kind(design::Kind::Outline)
        .enabled(view.can_test())
}

/// Record five seconds and hear them back, and the line under it.
fn record(view: &Huddle, cx: &mut Context<Huddle>, theme: &Theme) -> Div {
    let playback = view.mic_state.playback.clone();
    let under_way = playback != Playback::Idle;
    let icon = match under_way {
        true => STOP,
        false => PLAY,
    };
    let button = record_button(
        icon,
        words::record(&playback),
        view.can_record(),
        theme,
        cx.listener(move |view: &mut Huddle, _: &ClickEvent, _, cx| {
            match under_way {
                true => view.recorder.stop(),
                false => view.recorder.record(RECORD_SECONDS),
            }
            cx.notify();
        }),
    )
    .w_full()
    .justify_start();
    let line: AnyElement = match (&playback, under_way) {
        _ if view.muted => caption(words::record_line(true, &playback), theme),
        (Playback::Recording, true) => div()
            .id("huddle-record-line")
            .flex()
            .text_size(design::text::CAPTION)
            .text_color(theme.muted)
            .child(words::RECORDING)
            .child(
                div()
                    .font_family(design::fonts::FAMILY_MONO)
                    .child(view.countdown()),
            )
            .into_any_element(),
        _ => caption(words::record_line(false, &playback), theme),
    };
    div()
        .flex()
        .flex_col()
        .gap(design::space::XS)
        .child(button)
        .child(line)
}

fn caption(text: &'static str, theme: &Theme) -> AnyElement {
    div()
        .id("huddle-record-line")
        .text_size(design::text::CAPTION)
        .text_color(theme.muted)
        .child(text)
        .into_any_element()
}

/// Hear myself live: its switch, and the headphones warning always beside
/// it.
fn monitor(view: &Huddle, cx: &mut Context<Huddle>, theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(design::space::XS)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(design::text::SECONDARY)
                        .font_weight(FontWeight::MEDIUM)
                        .child(words::HEAR_MYSELF),
                )
                .child(design::switch(
                    "huddle-monitor",
                    words::HEAR_MYSELF,
                    view.monitor,
                    view.mic_live(),
                    // the desktop's sizes: a phone gets a tree of its own
                    &design::POINTER,
                    theme,
                    cx.listener(|view: &mut Huddle, _: &ClickEvent, _, cx| {
                        view.monitor = !view.monitor;
                        cx.notify();
                    }),
                )),
        )
        .child(
            div()
                .flex()
                .items_start()
                .gap(design::space::XS)
                .text_size(design::text::CAPTION)
                .text_color(theme.warning)
                .child(design::icon(HEADPHONES, px(13.)).mt(px(1.)))
                .child(div().flex_1().min_w(px(0.)).child(
                    "Wear headphones: through speakers your voice loops back into the \
                     microphone and howls.",
                )),
        )
}

// SDK gap: design::button takes no height, width or centred label (Join is
// 32 tall across the column), swap when feat/phone-kit lands
fn join_button(cx: &mut Context<Huddle>, theme: &Theme) -> Stateful<Div> {
    design::focus_shown_on_ink(
        div()
            .id(JOIN)
            .h(px(32.))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.primary)
            .text_color(theme.primary_foreground)
            .text_size(design::text::BODY)
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .role(Role::Button)
            .focusable()
            .on_click(
                cx.listener(|view: &mut Huddle, _: &ClickEvent, window, cx| join(view, window, cx)),
            )
            .child(words::JOIN),
        theme,
    )
}

/// Record: a 28 px control in a hairline box, its glyph before its label;
/// one that cannot act is faint and says so.
// SDK gap: design::button draws no glyph before its label, swap when
// feat/phone-kit lands
fn record_button(
    icon: &'static str,
    label: impl Into<SharedString>,
    enabled: bool,
    theme: &Theme,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let theme = *theme;
    let button = div()
        .id("huddle-record")
        .h(design::size::CONTROL)
        .px(design::space::MD)
        .flex()
        .items_center()
        .justify_center()
        .gap(design::space::XS)
        .border_1()
        .bg(theme.background)
        .text_size(design::text::SECONDARY)
        .whitespace_nowrap()
        .role(Role::Button)
        .child(design::icon(icon, px(14.)))
        .child(label.into());
    match enabled {
        true => button
            .border_color(theme.border_strong)
            .cursor_pointer()
            .hover(move |style| style.bg(theme.surface))
            .focusable()
            .on_click(click),
        false => button
            .border_color(theme.border)
            .text_color(theme.faint)
            .aria_disabled(true),
    }
}

// ---------- states ----------

/// The card in the middle of the picture: glyph, title, text, what fixes
/// it. No ground of its own: the picture's box is raised behind it.
fn state_card(
    view: &Huddle,
    device: Device,
    cx: &mut Context<Huddle>,
    theme: &Theme,
) -> Stateful<Div> {
    let state = view.state(device);
    let (title, text) = words::trouble(device, state);
    div()
        .id("huddle-camera-trouble")
        .absolute()
        .inset_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(design::space::SM)
        .p(design::space::BLOCK)
        .text_center()
        .child(design::icon(trouble_glyph(&state.status), px(20.)).text_color(theme.muted))
        .child(
            div()
                .text_size(design::text::BODY)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.foreground)
                .child(title),
        )
        .when(!text.is_empty(), |card| {
            card.child(
                div()
                    .max_w(px(240.))
                    .text_size(design::text::CAPTION)
                    .text_color(theme.muted)
                    .child(text),
            )
        })
        .child(actions(view, device, cx, theme))
}

/// What fixes a device's trouble: Open Settings where the host can open
/// them, Try again, and on the check, for a missing device, joining
/// without it.
fn actions(view: &Huddle, device: Device, cx: &mut Context<Huddle>, theme: &Theme) -> Div {
    let prefix = look::fixes_id(device);
    let open_settings =
        cx.listener(move |view: &mut Huddle, _: &ClickEvent, _, cx| view.open_settings(device, cx));
    let try_again = cx.listener(|view: &mut Huddle, _: &ClickEvent, _, cx| view.retry(cx));
    let without = cx.listener(move |view: &mut Huddle, _: &ClickEvent, window, cx| {
        view.do_without(device);
        join(view, window, cx)
    });
    let id = |what: &str| format!("{prefix}-{what}");
    let pointer = &design::POINTER;
    div()
        .flex()
        .items_center()
        .justify_center()
        .gap(design::space::SM)
        .when(view.settings_page(device).is_some(), |row| {
            let open = design::button(
                id("settings"),
                words::OPEN_SETTINGS,
                pointer,
                theme,
                open_settings,
            );
            row.child(open.kind(design::Kind::Outline))
        })
        .child(
            design::button(id("retry"), words::TRY_AGAIN, pointer, theme, try_again)
                .kind(design::Kind::Outline),
        )
        .when(view.can_do_without(device), |row| {
            let join = design::button(
                id("without"),
                words::join_without(device),
                pointer,
                theme,
                without,
            );
            row.child(join.kind(design::Kind::Quiet))
        })
}

// ---------- the room ----------

fn room(view: &Huddle, cx: &mut Context<Huddle>, theme: &Theme) -> Stateful<Div> {
    let compact = compact(view);
    let network = view.network();
    div()
        .id(ROOM)
        .size_full()
        .flex()
        .flex_col()
        // M mutes, V turns the camera off: from anywhere in the room
        .role(Role::Group)
        .aria_label(words::HUDDLE)
        .focusable()
        .focus_visible(|style| style)
        .consumes_keys(["m", "v"])
        .on_key_down(
            cx.listener(|view: &mut Huddle, event: &KeyDownEvent, _, cx| {
                if event.keystroke.modifiers.modified() {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "m" => view.toggle_mute(cx),
                    "v" => view.toggle_camera(cx),
                    _ => {}
                }
            }),
        )
        .children(people::voices(view, cx))
        .child(stage(view, compact, network.clone(), cx, theme))
        .when(!compact, |room| {
            room.children(network.map(|line| call::network_row(line_glyph(view), line, theme)))
                .child(bar(view, cx, theme))
        })
}

/// The stage: you alone fill it; with others, a grid of them and you, the
/// one speaking first, the rest past six a "+N" chip; in the compact room
/// the one speaking fills it, you a thumbnail over them.
fn stage(
    view: &Huddle,
    compact: bool,
    network: Option<String>,
    cx: &mut Context<Huddle>,
    theme: &Theme,
) -> Stateful<Div> {
    let (width, height) = view.viewport;
    let others = view
        .call
        .as_ref()
        .map(|call| call.order())
        .unwrap_or_default();
    let stage = div()
        .id("huddle-stage")
        .relative()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .map(|stage| match compact {
            true => stage.bg(black()),
            false => stage.p(design::space::LG).bg(theme.surface),
        });
    if compact {
        // the strip first: Try again and the lines' close come after its
        // controls in the keys' order
        let main = others.first().copied();
        let stage = match main {
            None => stage.child(tile(view, Shown::Alone, width, cx, theme)),
            Some(main) => stage
                .child(people::tile(view, main, width, cx, &design::POINTER, theme).size_full())
                .child(call::thumbnail(
                    tile(view, Shown::Thumbnail, width / 3., cx, theme),
                    width / 3.,
                ))
                .children((others.len() > 1).then(|| {
                    people::more(others.len() - 1, &design::POINTER)
                        .absolute()
                        .left(design::space::SM)
                        .top(design::space::SM)
                })),
        };
        return stage
            .child(float_strip(view, main, cx, theme))
            .children(call::knocks(view, cx, theme))
            .children(trouble_rows(view, cx, theme))
            .children(network.map(|line| call::network_glass(line_glyph(view), line)))
            .when(view.no_keep_on_top_line, |stage| {
                stage.child(no_keep_on_top_line(cx))
            });
    }
    let inner = (
        width - 2. * f(design::space::LG),
        height - f(BAR) - 2. * f(design::space::LG),
    );
    // the knocks after the tiles: what comes later draws over them
    if others.is_empty() {
        return stage
            .child(tile(view, Shown::Alone, inner.0, cx, theme))
            .children(call::knocks(view, cx, theme));
    }
    let shown = call::shown(others.len());
    let (cols, rows) = call::grid(shown + 1, inner.0, inner.1);
    let tile_width = call::cell(inner.0, cols);
    let tile_height = call::cell(inner.1, rows);
    let mut tiles: Vec<Stateful<Div>> = others[..shown]
        .iter()
        .map(|who| people::tile(view, *who, tile_width, cx, &design::POINTER, theme))
        .collect();
    tiles.push(tile(view, Shown::Grid, tile_width, cx, theme));
    let mut tiles = tiles.into_iter();
    let grid = div()
        .size_full()
        .flex()
        .flex_col()
        .gap(design::space::SM)
        .children((0..rows).map(|_| {
            div().flex().gap(design::space::SM).children(
                tiles
                    .by_ref()
                    .take(cols)
                    .map(|tile| tile.w(px(tile_width)).h(px(tile_height))),
            )
        }));
    stage
        .child(grid)
        .children((others.len() > shown).then(|| {
            people::more(others.len() - shown, &design::POINTER)
                .absolute()
                .right(design::space::LG + design::space::SM)
                .bottom(design::space::LG + design::space::SM)
        }))
        .children(call::knocks(view, cx, theme))
}

/// Where your tile is drawn: alone on the stage, one of a grid, or a
/// thumbnail over the one speaking.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shown {
    Alone,
    Grid,
    Thumbnail,
}

/// Your tile: the picture covering it (mirrored as you set), a ring while
/// you speak; alone, the chip with your marks and the line that says no one
/// else is here; in a grid, the chip; as a thumbnail, the muted badge.
fn tile(
    view: &Huddle,
    shown: Shown,
    width: f32,
    cx: &mut Context<Huddle>,
    theme: &Theme,
) -> Stateful<Div> {
    let off = view.camera_off;
    let troubled = view.troubled(Device::Camera);
    let speaking = view.speaking();
    let mut tile = div()
        .id("huddle-tile-you")
        .relative()
        .overflow_hidden()
        .role(Role::Group)
        .aria_label(words::YOU)
        .bg(match off || troubled {
            true => theme.surface_raised,
            false => black(),
        })
        .when(shown == Shown::Alone, |tile| tile.size_full());
    if view.camera_placed() {
        tile = tile.child(camera_piece(view, ObjectFit::Cover, cx).size_full());
    }
    // the microphone has nothing to draw in the room: it is there to be
    // heard, and to say who is speaking
    if view.mic_placed() {
        tile = tile.child(mic_piece(view, cx).absolute().size(px(0.)));
    }
    if off {
        let size = match (shown, width) {
            (Shown::Thumbnail, _) => 24.,
            (_, _) if compact(view) => 48.,
            (_, w) if w >= 300. => 72.,
            (_, w) if w >= 180. => 48.,
            _ => 32.,
        };
        tile = tile.child(look::face(px(size), theme));
    }
    let tile = tile.when(speaking, |tile| tile.child(people::ring(theme)));
    match shown {
        Shown::Thumbnail => {
            return tile.when(view.muted, |tile| {
                tile.child(
                    div()
                        .id("huddle-thumbnail-muted")
                        .absolute()
                        .left_0()
                        .bottom_0()
                        .size(px(16.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(theme.danger)
                        .text_color(white())
                        .role(Role::Image)
                        .aria_label(words::MUTED)
                        .child(design::icon(MIC_OFF, px(10.))),
                )
            });
        }
        Shown::Grid => {
            return tile
                .children(troubled.then(|| state_card(view, Device::Camera, cx, theme)))
                .child(
                    div()
                        .absolute()
                        .left(design::space::SM)
                        .bottom(design::space::SM)
                        .child(room_chip(view, speaking)),
                );
        }
        Shown::Alone if compact(view) => return tile,
        Shown::Alone => {}
    }
    // the chip and the line share the tile's foot: the line centred
    // between two equal sides, the chip's never narrower than the chip,
    // so in a float sized up to the room the line moves off it, wrapping
    // if it must
    tile.children(troubled.then(|| state_card(view, Device::Camera, cx, theme)))
        .child(
            div()
                .absolute()
                .left(design::space::SM)
                .right(design::space::SM)
                .bottom(design::space::SM)
                .flex()
                .items_end()
                .gap(design::space::SM)
                .child(div().flex_1().flex().child(room_chip(view, speaking)))
                .child(
                    div()
                        .id("huddle-solo")
                        .min_w(px(0.))
                        .px(design::space::SM)
                        .py(design::space::XXS)
                        .bg(glass())
                        .text_color(white())
                        .text_size(design::text::CAPTION)
                        .child(words::solo(view.call.is_some())),
                )
                .child(div().flex_1()),
        )
}

/// The microphone's control draws it off, slashed on the danger fill:
/// muted, or in trouble.
fn mic_off(view: &Huddle) -> bool {
    view.muted || view.troubled(Device::Microphone)
}

/// The muted or the speaking mark your chip carries, named for a reader.
fn mark(view: &Huddle, speaking: bool) -> Option<Stateful<Div>> {
    match (view.muted, speaking) {
        (true, _) => Some(
            div()
                .id("huddle-mark-muted")
                .flex()
                .role(Role::Image)
                .aria_label(words::MUTED)
                .child(design::icon(MIC_OFF, px(12.)).text_color(muted_mark())),
        ),
        (false, true) => Some(speaking_mark(white())),
        (false, false) => None,
    }
}

/// The speaking mark, named for a reader.
fn speaking_mark(color: Hsla) -> Stateful<Div> {
    div()
        .id("huddle-mark-speaking")
        .flex()
        .role(Role::Image)
        .aria_label(words::SPEAKING)
        .child(design::icon(SPEAKING, px(12.)).text_color(color))
}

/// The tile's chip: the muted or speaking mark, the name, "· mirrored".
fn room_chip(view: &Huddle, speaking: bool) -> Div {
    div()
        .flex_none()
        .h(px(22.))
        .px(design::space::XS)
        .flex()
        .items_center()
        .gap(design::space::XS)
        .bg(glass())
        .text_color(white())
        .text_size(design::text::CAPTION)
        .children(mark(view, speaking))
        .child(words::YOU)
        .when(view.camera_placed() && !view.unmirrored, |chip| {
            chip.child(
                div()
                    .font_family(design::fonts::FAMILY_MONO)
                    .opacity(0.7)
                    .child(words::MIRRORED),
            )
        })
}

/// The room's bar: its name and clock, the microphone and camera with
/// their menus, Invite, Leave, and who is here.
fn bar(view: &Huddle, cx: &mut Context<Huddle>, theme: &Theme) -> Stateful<Div> {
    div()
        .id("huddle-bar")
        .h(BAR)
        .flex_none()
        .px(design::space::LG)
        .flex()
        .items_center()
        .gap(design::space::LG)
        .border_t_1()
        .border_color(theme.border)
        .bg(theme.background)
        // the name and clock give way first: in a float sized down to
        // the room's 480 every control still fits
        .child(
            div()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(design::text::BODY)
                        .font_weight(FontWeight::MEDIUM)
                        .truncate()
                        .child(words::HUDDLE),
                )
                .child(
                    div()
                        .id("huddle-clock")
                        .font_family(design::fonts::FAMILY_MONO)
                        .text_size(design::text::CAPTION)
                        .text_color(theme.muted)
                        .truncate()
                        .child(words::in_room(&view.clock(), view.people_here())),
                ),
        )
        .child(split(view, Device::Microphone, cx, theme).ml_auto())
        .children(
            view.troubled(Device::Microphone)
                .then(|| retry(Device::Microphone, cx, theme)),
        )
        .child(split(view, Device::Camera, cx, theme))
        .child(call::invite(view, cx, theme))
        .child(call::link_or_start(view, cx, theme))
        .child(
            div()
                .id("huddle-leave")
                .flex_none()
                .h(px(32.))
                .px(design::space::LG)
                .flex()
                .items_center()
                .gap(design::space::XS)
                .bg(theme.danger)
                .text_color(white())
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .role(Role::Button)
                .focusable()
                .on_click(
                    cx.listener(|view: &mut Huddle, _: &ClickEvent, window, cx| {
                        leave(view, window, cx)
                    }),
                )
                .child(design::icon(LEAVE, px(14.)))
                .child(words::LEAVE),
        )
        .child(
            div()
                .ml_auto()
                .flex_none()
                .flex()
                .items_center()
                .gap(design::space::SM)
                .child(
                    div()
                        .id("huddle-count")
                        .flex()
                        .items_center()
                        .gap(design::space::XXS)
                        .text_color(theme.muted)
                        .role(Role::Group)
                        .aria_label(words::in_the_huddle(view.people_here()))
                        .child(design::icon(PEOPLE, px(14.)))
                        .child(
                            div()
                                .font_family(design::fonts::FAMILY_MONO)
                                .text_size(design::text::CAPTION)
                                .child(view.people_here().to_string()),
                        ),
                )
                .children(pin_back(view, false, cx, theme)),
        )
}

/// The pin and back after the bar's count, or at the float strip's end:
/// floating, the pin keeps the window on top or not (none where the host
/// keeps nothing on top) and back returns it to the full window; on the
/// desk the pin floats it again. Neither where the host cannot float.
fn pin_back(
    view: &Huddle,
    glassy: bool,
    cx: &mut Context<Huddle>,
    theme: &Theme,
) -> Vec<Stateful<Div>> {
    let floating = view.placement.floating;
    let on_top = floating && view.placement.on_top;
    let pin = (!view.no_float && (!floating || !view.no_keep_on_top)).then(|| {
        small(
            ("huddle-pin", if on_top { PIN } else { PIN_OFF }),
            if floating {
                "Keep on top"
            } else {
                "Float this window"
            },
            glassy,
            false,
            theme,
            cx.listener(|view: &mut Huddle, _: &ClickEvent, _, cx| {
                view.no_keep_on_top_line = false;
                view.pin(cx)
            }),
        )
        .aria_toggled(on_top.into())
    });
    let back = floating.then(|| {
        small(
            ("huddle-back", BACK),
            "Back to the full window",
            glassy,
            false,
            theme,
            cx.listener(|view: &mut Huddle, _: &ClickEvent, _, cx| view.back(cx)),
        )
    });
    pin.into_iter().chain(back).collect()
}

/// A 28 px control: glass over the picture (`glassy`), else the bar's
/// hairline box; danger-filled while `off`.
fn small(
    (id, icon): (&'static str, &'static str),
    name: &'static str,
    glassy: bool,
    off: bool,
    theme: &Theme,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let theme = *theme;
    div()
        .id(id)
        .size(design::size::CONTROL)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .map(|button| match (off, glassy) {
            (true, _) => button.bg(theme.danger).text_color(white()),
            (false, true) => button.bg(glass()).text_color(white()),
            (false, false) => button
                .border_1()
                .border_color(theme.border_strong)
                .text_color(theme.foreground)
                .hover(move |style| style.bg(theme.surface)),
        })
        .role(Role::Button)
        .aria_label(name)
        .focusable()
        .on_click(click)
        .child(design::icon(icon, px(14.)))
}

// ---------- the compact room ----------

/// The compact room's one row over the stage's foot: the chip of whoever
/// fills it (`main`, else you), the microphone, the camera and Leave, then
/// the pin and back. Always shown: a small window whose controls hide is a
/// guessing game, and a keyboard or a screen reader never hovers.
fn float_strip(
    view: &Huddle,
    main: Option<u64>,
    cx: &mut Context<Huddle>,
    theme: &Theme,
) -> Stateful<Div> {
    let theme_ = *theme;
    let (name, mark) = match main {
        None => (words::YOU.to_owned(), mark(view, view.speaking())),
        Some(who) => {
            let speaking = view
                .call
                .as_ref()
                .and_then(|call| call.people.get(&who))
                .is_some_and(|person| person.voice.speaking);
            (view.name(who), speaking.then(|| speaking_mark(white())))
        }
    };
    let chip = div()
        .h(design::size::CONTROL)
        .min_w(px(0.))
        .flex_shrink(1.)
        .px(design::space::XS)
        .flex()
        .items_center()
        .gap(design::space::XS)
        .bg(glass())
        .text_color(white())
        .text_size(design::text::CAPTION)
        .children(mark)
        .child(div().min_w(px(0.)).truncate().child(name));
    let controls = div()
        .flex()
        .flex_none()
        .gap(design::space::XS)
        .child(
            small(
                ("huddle-mic", if mic_off(view) { MIC_OFF } else { MIC }),
                words::mute(view.muted),
                true,
                mic_off(view),
                &theme_,
                cx.listener(|view: &mut Huddle, _: &ClickEvent, _, cx| view.toggle_mute(cx)),
            )
            .aria_toggled(view.muted.into()),
        )
        .child(
            small(
                (
                    "huddle-camera",
                    if view.camera_off { VIDEO_OFF } else { VIDEO },
                ),
                words::camera_toggle(view.camera_off),
                true,
                view.camera_off,
                &theme_,
                cx.listener(|view: &mut Huddle, _: &ClickEvent, _, cx| view.toggle_camera(cx)),
            )
            .aria_toggled(view.camera_off.into()),
        )
        .child(small(
            ("huddle-leave", LEAVE),
            words::LEAVE,
            true,
            true,
            &theme_,
            cx.listener(|view: &mut Huddle, _: &ClickEvent, window, cx| leave(view, window, cx)),
        ));
    div()
        .id("huddle-float-strip")
        .absolute()
        .left(design::space::SM)
        .right(design::space::SM)
        .bottom(design::space::SM)
        .h(design::size::CONTROL)
        .flex()
        .items_center()
        .child(chip)
        .child(div().flex_1().min_w(design::space::SM))
        .child(controls)
        .child(div().flex_1().min_w(design::space::SM))
        .child(
            div()
                .flex()
                .flex_none()
                .gap(design::space::XS)
                .children(pin_back(view, true, cx, theme)),
        )
}

/// The compact room's troubles in the tile's middle, the camera's over
/// the microphone's, each its card as one row.
fn trouble_rows(view: &Huddle, cx: &mut Context<Huddle>, theme: &Theme) -> Option<Div> {
    let rows: Vec<Div> = [Device::Camera, Device::Microphone]
        .into_iter()
        .filter(|device| view.troubled(*device))
        .map(|device| trouble_row(view, device, cx, theme))
        .collect();
    (!rows.is_empty()).then(|| {
        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(design::space::SM)
            .children(rows)
    })
}

/// A device's trouble as one row: its glyph, its title and Try again.
fn trouble_row(view: &Huddle, device: Device, cx: &mut Context<Huddle>, theme: &Theme) -> Div {
    let state = view.state(device);
    let (title, _) = words::trouble(device, state);
    div()
        .flex()
        .items_center()
        .gap(design::space::SM)
        .child(design::icon(trouble_glyph(&state.status), px(16.)).text_color(theme.muted))
        .child(
            div()
                .text_size(design::text::BODY)
                .text_color(theme.foreground)
                .child(title),
        )
        .child(retry(device, cx, theme))
}

/// Try again, quiet: both pieces opened again, the lists read again.
fn retry(device: Device, cx: &mut Context<Huddle>, theme: &Theme) -> impl IntoElement {
    let id = match device {
        Device::Camera => "huddle-camera-retry",
        Device::Microphone => "huddle-microphone-retry",
    };
    let try_again = cx.listener(|view: &mut Huddle, _: &ClickEvent, _, cx| view.retry(cx));
    design::button(id, words::TRY_AGAIN, &design::POINTER, theme, try_again)
        .kind(design::Kind::Quiet)
}

/// What the float says once where the host keeps no window on top
/// (Wayland), over the strip, until closed or the next press.
fn no_keep_on_top_line(cx: &mut Context<Huddle>) -> Div {
    div()
        .absolute()
        .left(design::space::SM)
        .right(design::space::SM)
        .bottom(design::space::SM + design::size::CONTROL + design::space::XS)
        .h(px(22.))
        .px(design::space::XS)
        .flex()
        .items_center()
        .gap(design::space::XS)
        .bg(glass())
        .text_color(white())
        .text_size(design::text::CAPTION)
        .child(design::icon(PIN_OFF, px(12.)))
        .child(
            div()
                .id("huddle-no-keep-on-top")
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .child("Wayland: no keep-on-top here."),
        )
        .child(
            div()
                .id("huddle-no-keep-on-top-close")
                .size(px(22.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .role(Role::Button)
                .aria_label("Close")
                .focusable()
                .on_click(cx.listener(|view: &mut Huddle, _: &ClickEvent, _, cx| {
                    view.no_keep_on_top_line = false;
                    cx.notify();
                }))
                .child(design::icon(CLOSE, px(12.))),
        )
}

/// A device's split control in the room's bar: its toggle, and the caret
/// that opens its menu upward (the microphone's lists the speakers too).
/// The toggle is danger-filled while off; the microphone's while in trouble
/// too, its trouble told to assistive technology (the camera's trouble is
/// its tile's card).
fn split(view: &Huddle, device: Device, cx: &mut Context<Huddle>, theme: &Theme) -> view::Sensor {
    let theme = *theme;
    let (id, name, off, alarm, which, caret_id, caret_name) = match device {
        Device::Microphone => (
            "huddle-mic",
            words::mute(view.muted),
            view.muted,
            mic_off(view),
            Which::Sound,
            "huddle-mic-menu",
            "Choose microphone and speaker",
        ),
        Device::Camera => (
            "huddle-camera",
            words::camera_toggle(view.camera_off),
            view.camera_off,
            view.camera_off,
            Which::Kind(Kind::Camera),
            "huddle-camera-menu",
            "Choose camera",
        ),
    };
    let icon = match (device, alarm) {
        (Device::Microphone, true) => MIC_OFF,
        (Device::Microphone, false) => MIC,
        (Device::Camera, true) => VIDEO_OFF,
        (Device::Camera, false) => VIDEO,
    };
    let trouble = (device == Device::Microphone && view.troubled(device))
        .then(|| words::trouble(device, &view.mic_state).0);
    let expanded = view.menu.is_some_and(|menu| menu.which == which);
    let caret = div()
        .id(caret_id)
        .w(px(18.))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .border_l_1()
        .border_color(theme.border_strong)
        .text_color(theme.muted)
        .cursor_pointer()
        .hover(move |style| style.bg(theme.surface))
        .role(Role::Button)
        .aria_label(caret_name)
        .aria_has_popup(MENU_POPUP)
        .aria_expanded(expanded)
        .focusable()
        .on_click(
            cx.listener(move |view: &mut Huddle, _: &ClickEvent, window, cx| {
                open(view, which, caret_id, window, cx)
            }),
        )
        .child(design::icon(CARET, px(12.)));
    let split = div()
        .flex()
        .flex_none()
        .h(px(32.))
        .border_1()
        .border_color(theme.border_strong)
        .child(
            div()
                .id(id)
                .w(px(36.))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .map(|main| match alarm {
                    true => main.bg(theme.danger).text_color(white()),
                    false => main
                        .text_color(theme.foreground)
                        .hover(move |style| style.bg(theme.surface)),
                })
                .role(Role::Button)
                .aria_label(name)
                .when_some(trouble, |main, trouble| main.aria_description(trouble))
                .aria_toggled(off.into())
                .focusable()
                .on_click(cx.listener(
                    move |view: &mut Huddle, _: &ClickEvent, _, cx| match device {
                        Device::Microphone => view.toggle_mute(cx),
                        Device::Camera => view.toggle_camera(cx),
                    },
                ))
                .child(design::icon(icon, px(16.))),
        )
        .child(caret);
    anchor(which, caret_id, split, cx)
}

// ---------- the device menu ----------

/// A line of an open menu: a group's heading (the room's sound menu has
/// two), or row `index` of its devices. Every line is a row tall, so the
/// menu is one uniform list.
#[derive(Clone, Copy)]
enum Line {
    Head(Kind),
    Row(usize),
}

/// The lines of the menu of `which` over its `rows`: the sound menu heads
/// each kind's rows.
fn lines(which: Which, rows: &[Row]) -> Vec<Line> {
    let mut lines = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if which == Which::Sound && (index == 0 || rows[index - 1].kind != row.kind) {
            lines.push(Line::Head(row.kind));
        }
        lines.push(Line::Row(index));
    }
    lines
}

/// Row `index` of the menu of `which` scrolled into its list.
fn reveal(view: &Huddle, which: Which, index: usize, strategy: ScrollStrategy) {
    let lines = lines(which, &view.rows(which));
    let line = lines
        .iter()
        .position(|line| matches!(line, Line::Row(at) if *at == index));
    if let Some(line) = line {
        view.desk.menu_scroll.scroll_to_item(line, strategy);
    }
}

/// The arrows on row `index` of the open menu: it is active, and scrolled
/// to before the frame that claims it.
fn step(view: &mut Huddle, index: usize, cx: &mut Context<Huddle>) {
    let Some(menu) = &mut view.menu else {
        return;
    };
    menu.cursor = index;
    let which = menu.which;
    reveal(view, which, index, ScrollStrategy::Nearest);
    cx.notify();
}

/// A group's heading in the sound menu.
fn heading(kind: Kind, theme: &Theme) -> Div {
    div()
        .h(design::size::ROW)
        .flex_none()
        .px(design::space::SM)
        .flex()
        .items_center()
        .text_size(design::text::CAPTION)
        .text_color(theme.muted)
        .child(words::kind(kind))
}

/// The open device menu: one Tab stop whose ↑ ↓ walk the rows, Enter or
/// Space picks, Escape closes; under its picker on the check, over its
/// caret in the room.
fn menu(view: &Huddle, open: Menu, cx: &mut Context<Huddle>, theme: &Theme) -> AnyElement {
    let theme = *theme;
    let rows = view.rows(open.which);
    let count = rows.len();
    let active = open.cursor.min(count.saturating_sub(1));
    let lines = lines(open.which, &rows);
    let total = lines.len();
    let pick = Rc::new(cx.processor(|view: &mut Huddle, index: usize, window, cx| {
        closed(view.pick(index, cx), window)
    }));
    let pressed = pick.clone();
    let paint = move |range: Range<usize>, _: &mut Window, _: &mut App| {
        range
            .map(|at| match lines[at] {
                // the second group ruled off the first
                Line::Head(kind) => heading(kind, &theme)
                    .when(at > 0, |head| head.border_t_1().border_color(theme.border))
                    .into_any_element(),
                Line::Row(index) => {
                    let pick = pick.clone();
                    let click = move |_: &ClickEvent, window: &mut Window, app: &mut App| {
                        pick(index, window, app)
                    };
                    let row = menu_row(index, &rows[index], index == active, click, &theme);
                    design::item(row, Role::MenuItemRadio, index == active).into_any_element()
                }
            })
            .collect()
    };
    let escape = cx.listener(|view: &mut Huddle, event: &KeyDownEvent, window, cx| {
        if event.keystroke.key == "escape" && !event.keystroke.modifiers.modified() {
            closed(view.close_menu(cx), window);
        }
    });
    let frame = design::composite(MENU, Role::Menu, words::menu(open.which))
        .active(active, count)
        .on_move(cx.processor(|view: &mut Huddle, index: usize, _, cx| step(view, index, cx)))
        .on_press(move |index, window, app| pressed(index, window, app))
        .build()
        // Escape is the menu's: the overlay around it does not close it again
        .on_key_down(escape)
        .consumes_keys(["escape"])
        .w(px(COLUMN))
        .py(design::space::XXS)
        .flex()
        .flex_col()
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.background)
        .shadow_lg()
        .occlude()
        .child(
            uniform_list("huddle-menu-rows", total, paint)
                .track_scroll(&view.desk.menu_scroll)
                .w_full()
                .h(design::size::ROW * total.min(MENU_LINES) as f32),
        );
    // the ring would replace the shadow: the frame draws both
    let frame = design::focus_shown(frame, &theme, |style| style.shadow_lg());
    let (at, side) = place(view, open.which);
    anchored()
        .position_mode(AnchoredPositionMode::Window)
        .position(at)
        .anchor(side)
        .snap_to_window_with_margin(Edges::all(design::space::SM))
        .child(frame)
        .into_any_element()
}

/// Where a menu hangs: under its picker's left edge on the check; over its
/// split's left edge, a little above, in the room. Before layout told the
/// opener's bounds, the layout's own geometry: under the column's first
/// picker, or over the middle of the bar.
fn place(view: &Huddle, which: Which) -> (Point<Pixels>, Anchor) {
    let (width, height) = view.viewport;
    match (view.screen, view.desk.anchor(which)) {
        (Screen::Check, Some(bounds)) => (bounds.bottom_left(), Anchor::TopLeft),
        (Screen::Room, Some(bounds)) => (
            Point {
                x: bounds.origin.x,
                y: bounds.origin.y - design::space::XS,
            },
            Anchor::BottomLeft,
        ),
        (Screen::Check, None) => (
            Point {
                x: px(width - COLUMN) - design::space::BLOCK,
                y: design::space::BLOCK + FIELD_LABEL + design::space::XXS + design::size::CONTROL,
            },
            Anchor::TopLeft,
        ),
        (Screen::Room, None) => (
            Point {
                x: px(width / 2.),
                y: px(height) - BAR,
            },
            Anchor::BottomLeft,
        ),
    }
}

fn menu_row(
    index: usize,
    row: &Row,
    active: bool,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    theme: &Theme,
) -> Stateful<Div> {
    let theme = *theme;
    let empty = row.id.is_none();
    div()
        .id(SharedString::from(format!("huddle-menu-{index}")))
        .h(design::size::ROW)
        .flex_none()
        .px(design::space::SM)
        .flex()
        .items_center()
        .gap(design::space::SM)
        .text_size(design::text::SECONDARY)
        .text_color(match row.picked || active {
            true => theme.foreground,
            false => theme.muted,
        })
        .when(active && !empty, |row| row.bg(theme.surface_raised))
        .aria_toggled(row.picked.into())
        .map(|line| match empty {
            true => line.aria_disabled(true),
            false => line
                .cursor_pointer()
                .hover(move |style| style.bg(theme.surface_raised))
                .on_click(click),
        })
        .child(match row.picked {
            true => design::icon(CHECK, px(14.))
                .text_color(theme.foreground)
                .into_any_element(),
            false => div().size(px(14.)).flex_none().into_any_element(),
        })
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .child(row.name.clone()),
        )
        .children(row.twin.clone().map(|id| {
            div()
                .flex_none()
                .font_family(design::fonts::FAMILY_MONO)
                .text_size(design::text::CAPTION)
                .text_color(theme.faint)
                .child(id)
        }))
}

// ---------- a build without camera and microphone ----------

fn unsupported(theme: &Theme) -> Stateful<Div> {
    div()
        .id("huddle-unsupported-screen")
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(design::space::SM)
        .text_center()
        .child(design::icon(VIDEO_OFF, px(20.)).text_color(theme.foreground))
        .child(
            design::empty_state(
                "huddle-unsupported",
                VIDEO_OFF,
                words::UNSUPPORTED,
                words::UNSUPPORTED_DETAIL,
                &design::POINTER,
                theme,
            )
            .items_center()
            .pt_0(),
        )
}
