//! The room (phone board 30, `design-phone/SPEC.md` §2; no Expo source: a
//! new screen in the Expo kit's look). Its title row holds the clock, who
//! is here and Invite; alone, your tile fills the stage, the picture
//! covering it, with the chip, a ring while you speak, and the line that
//! says no one else is here; in a call, the one speaking fills it and you
//! are a thumbnail over them, the rest a "+N" chip, as the desktop's float
//! has it. Under the stage, in the thumb's reach, the bar: the microphone,
//! the camera, the flip, the Sound route and Leave. The view stays where
//! it is (a phone keeps one window): no pin, no float.
//!
//! Beside Invite in the title row: Start a call with no call, Copy link in
//! one. The knocks at the door are a strip over the stage, each with Let in
//! and Turn away.
//!
//! Reads the picks and switches, what the pieces told, the call's people,
//! its door and the room's clock. Writes the toggles, the flip, a retry or
//! the system's settings for a device in trouble, the sound and invite
//! sheets' opening, a call's start, its link, a knock's answer and the
//! check (`leave_room`).
use view::design::{IconButton, Ladder};
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use super::SOUND;
use super::parts::{self, FLIP};
use crate::look::{self, LEAVE, MIC, MIC_OFF, SPEAKER, VIDEO, VIDEO_OFF, black, glass, white};
use crate::people;
use crate::words::{self, Device};
use crate::{Huddle, Kind, Which};

// the title row's glyphs, by the Lucide names `design::icon` draws
const INVITE: &str = "user-plus";
const START: &str = "phone";
const LINK: &str = "link";
const COPIED: &str = "check";

/// Your face while the camera is off (SPEC §2.1), and in your thumbnail.
const FACE: Pixels = px(72.);
const THUMBNAIL_FACE: Pixels = px(24.);

/// The bar's buttons, square in the thumb's reach (SPEC §2).
const BAR_BUTTON: Pixels = px(56.);

/// How far over the tile's foot the solo line sits, and how wide it runs
/// (SPEC §2.1).
const SOLO_ABOVE: Pixels = px(68.);
const SOLO_WIDTH: Pixels = px(320.);

pub(super) fn screen(
    view: &Huddle,
    cx: &mut Context<Huddle>,
    ladder: &Ladder,
    theme: &Theme,
) -> impl IntoElement {
    // the check's page, in the room: the host's capsule closes it
    design::page(
        "huddle-room",
        words::HUDDLE,
        design::Nav::Child,
        ladder,
        theme,
    )
    .caption(words::in_room(&view.clock(), view.people_here()))
    .trailing(link_or_start(view, cx, ladder, theme))
    .trailing(invite(cx, ladder, theme))
    .children(people::voices(view, cx))
    .children(knocks(view, cx, ladder, theme))
    .child(stage(view, cx, ladder, theme))
    .child(bar(view, cx, ladder, theme))
}

/// Invite, at the title row's end: the people to ring, in a sheet.
fn invite(cx: &mut Context<Huddle>, ladder: &Ladder, theme: &Theme) -> impl IntoElement {
    let glyph = design::icon(INVITE, ladder.icon);
    let open = cx.listener(|view: &mut Huddle, _: &ClickEvent, _, cx| {
        view.open_menu(Which::Invite, "huddle-invite", cx)
    });
    design::icon_button(
        "huddle-invite",
        glyph,
        words::INVITE_PEOPLE,
        ladder,
        theme,
        open,
    )
}

/// Beside Invite: Start a call with no call (one with no one rung, whose
/// link brings people to its door); Copy link in one, its glyph a check
/// for a while once copied.
fn link_or_start(
    view: &Huddle,
    cx: &mut Context<Huddle>,
    ladder: &Ladder,
    theme: &Theme,
) -> impl IntoElement {
    let (id, glyph, name) = match (view.call.is_some(), view.just_copied()) {
        (false, _) => ("huddle-start", START, words::START_CALL),
        (true, false) => ("huddle-copy-link", LINK, words::COPY_LINK),
        (true, true) => ("huddle-copy-link", COPIED, words::LINK_COPIED),
    };
    let press = cx.listener(
        |view: &mut Huddle, _: &ClickEvent, _, cx| match view.call.is_some() {
            true => view.copy_link(cx),
            false => view.start_alone(cx),
        },
    );
    design::icon_button(
        id,
        design::icon(glyph, ladder.icon),
        name,
        ladder,
        theme,
        press,
    )
}

/// The knocks at the door, a strip over the stage: each one's face and
/// name, Let in and Turn away.
fn knocks(
    view: &Huddle,
    cx: &mut Context<Huddle>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<Stateful<Div>> {
    let knocks = &view.call.as_ref()?.knocks;
    if knocks.is_empty() {
        return None;
    }
    let rows = knocks.iter().map(|who| {
        let who = *who;
        let name = view.name(who);
        let admit =
            cx.listener(move |view: &mut Huddle, _: &ClickEvent, _, cx| view.admit(who, cx));
        let turn_away =
            cx.listener(move |view: &mut Huddle, _: &ClickEvent, _, cx| view.turn_away(who, cx));
        div()
            .id(SharedString::from(format!("huddle-knock-{who}")))
            .flex()
            .items_center()
            .gap(design::space::SM)
            .px(design::space::BLOCK)
            .py(design::space::SM)
            .role(Role::Alert)
            .aria_live(view::accesskit::Live::Polite)
            .child(design::avatar(&name, ladder.avatar, theme))
            .child(
                // wraps: a long name keeps its words
                div()
                    .flex_1()
                    .min_w_0()
                    .text_type(ladder.secondary)
                    .font_weight(FontWeight::MEDIUM)
                    .child(words::asking_to_join(&name)),
            )
            .child(
                design::button(
                    format!("huddle-admit-{who}"),
                    words::LET_IN,
                    ladder,
                    theme,
                    admit,
                )
                .small()
                .kind(design::Kind::Primary),
            )
            .child(
                design::button(
                    format!("huddle-turn-away-{who}"),
                    words::TURN_AWAY,
                    ladder,
                    theme,
                    turn_away,
                )
                .small(),
            )
    });
    Some(
        div()
            .id("huddle-knocks")
            .flex_none()
            .flex()
            .flex_col()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .children(rows),
    )
}

/// The stage: a microphone in trouble said over it with what fixes it
/// (the room has no Sound row to say it in), the network's line, and your
/// tile; in a call with others, the one speaking, you a thumbnail over
/// them.
fn stage(view: &Huddle, cx: &mut Context<Huddle>, ladder: &Ladder, theme: &Theme) -> Stateful<Div> {
    let mic_trouble = view
        .troubled(Device::Microphone)
        .then(|| mic_trouble(view, cx, ladder, theme));
    let others = view
        .call
        .as_ref()
        .map(|call| call.order())
        .unwrap_or_default();
    let stage = div()
        .id("huddle-stage")
        .relative()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .children(mic_trouble)
        .children(network(view, ladder, theme));
    let Some(main) = others.first().copied() else {
        return stage.child(tile(view, true, cx, ladder, theme));
    };
    let (width, _) = view.viewport;
    let thumbnail = width / 3.;
    stage
        .child(
            people::tile(view, main, width, cx, ladder, theme)
                .flex_1()
                .min_h_0(),
        )
        .child(
            tile(view, false, cx, ladder, theme)
                .absolute()
                .top(design::space::LG)
                .right(design::space::LG)
                .w(px(thumbnail))
                .h(px(thumbnail * 16. / 9.))
                .border_1()
                .border_color(hsla(0., 0., 1., 0.25)),
        )
        .children((others.len() > 1).then(|| {
            people::more(others.len() - 1, ladder)
                .absolute()
                .left(design::space::LG)
                .top(design::space::LG)
        }))
}

/// The room's line over the stage, in the warning tone: why the call's
/// last op did not go through, its tracks not through, or the camera
/// sending less picture.
fn network(view: &Huddle, ladder: &Ladder, theme: &Theme) -> Option<Stateful<Div>> {
    let line = view.network()?;
    Some(
        div()
            .id("huddle-network")
            .flex_none()
            .flex()
            .items_center()
            .gap(design::space::SM)
            .px(design::space::BLOCK)
            .py(design::space::SM)
            .text_type(ladder.secondary)
            .text_color(theme.warning)
            .role(Role::Status)
            .aria_live(view::accesskit::Live::Polite)
            .child(design::icon(look::line_glyph(view), ladder.icon_row))
            .child(div().min_w_0().truncate().child(line)),
    )
}

/// The microphone's trouble, in the danger tone over the stage: its line
/// (the check's Sound row's), and what fixes it under the line. An alert,
/// said at once (the SDK notice's shape): a microphone failing mid-call is
/// heard, not only seen.
fn mic_trouble(
    view: &Huddle,
    cx: &mut Context<Huddle>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id("huddle-mic-trouble")
        .flex_none()
        .flex()
        .flex_col()
        .gap(design::space::SM)
        .px(design::space::BLOCK)
        .py(design::space::LG)
        .bg(theme.danger_soft)
        .border_b_1()
        .border_color(theme.danger)
        .role(Role::Alert)
        .aria_live(view::accesskit::Live::Assertive)
        .aria_live(view::accesskit::Live::Assertive)
        .child(
            div()
                .text_type(ladder.secondary)
                .text_color(theme.danger)
                .child(words::phone_line(Device::Microphone, &view.mic_state)),
        )
        .child(parts::fixes(view, Device::Microphone, cx, ladder, theme))
}

/// Your tile: the picture covering it, mirrored as the model says (or
/// your face, the camera off), the camera's card, a ring while you speak,
/// the chip; `alone`, filling the stage with the solo line, else a
/// thumbnail the caller sizes.
fn tile(
    view: &Huddle,
    alone: bool,
    cx: &mut Context<Huddle>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let off = view.camera_off;
    let speaking = view.speaking();
    div()
        .id("huddle-tile-you")
        .relative()
        .when(alone, |tile| tile.flex_1().min_h_0())
        .overflow_hidden()
        .role(Role::Group)
        .aria_label(words::YOU)
        .bg(match off || view.troubled(Device::Camera) {
            true => theme.surface_raised,
            false => black(),
        })
        .when(view.camera_placed(), |tile| {
            tile.child(parts::camera_piece(view, cx).size_full())
        })
        // the microphone has nothing to draw in the room: it is there to
        // be heard, and to say who is speaking
        .when(view.mic_placed(), |tile| {
            tile.child(parts::mic_piece(view, cx).absolute().size(px(0.)))
        })
        .when(off, |tile| {
            tile.child(look::face(if alone { FACE } else { THUMBNAIL_FACE }, theme))
        })
        .when(alone, |tile| {
            tile.children(parts::state_card(view, cx, ladder, theme))
        })
        .when(speaking, |tile| tile.child(people::ring(theme)))
        .when(alone, |tile| {
            tile.child(parts::chip(view, speaking, ladder, theme))
                .child(solo(ladder))
        })
}

/// The room says when no one else is in it.
fn solo(ladder: &Ladder) -> Div {
    div()
        .absolute()
        .left(design::space::LG)
        .right(design::space::LG)
        .bottom(SOLO_ABOVE)
        .flex()
        .justify_center()
        .child(
            div()
                .id("huddle-solo")
                .max_w(SOLO_WIDTH)
                .px(design::space::SM)
                .py(design::space::XXS)
                .bg(glass())
                .text_color(white())
                .text_type(ladder.caption)
                .text_center()
                .child(words::SOLO),
        )
}

/// The bar: the microphone, the camera, the flip and the Sound route, each
/// a square, and Leave taking the rest.
fn bar(view: &Huddle, cx: &mut Context<Huddle>, ladder: &Ladder, theme: &Theme) -> Stateful<Div> {
    // the host lays the view out above the home bar: nothing under the bar
    div()
        .id("huddle-bar")
        .flex_none()
        .flex()
        .gap(design::space::SM)
        .pt(design::space::LG)
        .px(design::space::BLOCK)
        .border_t_1()
        .border_color(theme.border)
        .bg(theme.background)
        .child(mic_toggle(view, cx, ladder, theme))
        .child(camera_toggle(view, cx, ladder, theme))
        .child(flip(view, cx, ladder, theme))
        .child(sound(view, cx, ladder, theme))
        .child(leave(cx, ladder, theme))
}

/// A square of the bar (SPEC §2), the kit's glyph button grown to the
/// thumb's reach: its glyph in a strong hairline, the danger fill while
/// `off`.
fn square<F>(button: IconButton<F>, off: bool, theme: &Theme) -> IconButton<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    let button = button.size(BAR_BUTTON);
    match off {
        true => button.bg(theme.danger).text_color(white()),
        false => button.border_1().border_color(theme.border_strong),
    }
}

/// The microphone: slashed on the danger fill while muted or in trouble
/// (the line over the stage says what the trouble is).
fn mic_toggle(
    view: &Huddle,
    cx: &mut Context<Huddle>,
    ladder: &Ladder,
    theme: &Theme,
) -> impl IntoElement {
    let off = view.muted || view.troubled(Device::Microphone);
    let glyph = design::icon(if off { MIC_OFF } else { MIC }, ladder.icon);
    let toggle = cx.listener(|view: &mut Huddle, _: &ClickEvent, _, cx| view.toggle_mute(cx));
    let name = words::mute(view.muted);
    let button = design::icon_button("huddle-mic", glyph, name, ladder, theme, toggle);
    square(button.toggled(view.muted), off, theme)
}

fn camera_toggle(
    view: &Huddle,
    cx: &mut Context<Huddle>,
    ladder: &Ladder,
    theme: &Theme,
) -> impl IntoElement {
    let off = view.camera_off;
    let glyph = design::icon(if off { VIDEO_OFF } else { VIDEO }, ladder.icon);
    let toggle = cx.listener(|view: &mut Huddle, _: &ClickEvent, _, cx| view.toggle_camera(cx));
    let name = words::camera_toggle(off);
    let button = design::icon_button("huddle-camera", glyph, name, ladder, theme, toggle);
    square(button.toggled(off), off, theme)
}

/// The flip to the camera on the other side; standing still while the
/// camera is off, or where the host lists no camera on the other side.
fn flip(
    view: &Huddle,
    cx: &mut Context<Huddle>,
    ladder: &Ladder,
    theme: &Theme,
) -> impl IntoElement {
    let other = view.other_camera().map(|camera| camera.facing);
    let glyph = design::icon(FLIP, ladder.icon);
    let flip = cx.listener(|view: &mut Huddle, _: &ClickEvent, _, cx| view.flip(cx));
    let name = words::flip(other.unwrap_or(Facing::Back));
    let button = design::icon_button("huddle-flip", glyph, name, ladder, theme, flip);
    square(
        button.enabled(other.is_some() && !view.camera_off),
        false,
        theme,
    )
}

/// The Sound route, named with it: it opens the sound sheet.
fn sound(
    view: &Huddle,
    cx: &mut Context<Huddle>,
    ladder: &Ladder,
    theme: &Theme,
) -> impl IntoElement {
    let glyph = design::icon(SPEAKER, ladder.icon);
    let open = cx.listener(|view: &mut Huddle, _: &ClickEvent, _, cx| {
        view.open_menu(SOUND, "huddle-sound", cx)
    });
    let name = words::sound(&view.shown_device(Kind::Speaker));
    let button = design::icon_button("huddle-sound", glyph, name, ladder, theme, open);
    square(button, false, theme)
}

/// Leave: back to the check.
fn leave(cx: &mut Context<Huddle>, ladder: &Ladder, theme: &Theme) -> Stateful<Div> {
    let leave = div()
        .id("huddle-leave")
        .flex_1()
        .min_w_0()
        .h(BAR_BUTTON)
        .flex()
        .items_center()
        .justify_center()
        .gap(design::space::SM)
        .bg(theme.danger)
        .text_color(white())
        .text_type(ladder.body)
        .font_weight(FontWeight::MEDIUM)
        .role(Role::Button)
        .child(design::icon(LEAVE, ladder.icon_row))
        .child(words::LEAVE);
    design::pressed(leave, ladder)
        .focusable()
        .on_click(cx.listener(|view: &mut Huddle, _: &ClickEvent, _, cx| view.leave_room(cx)))
}
