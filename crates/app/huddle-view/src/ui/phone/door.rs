//! The door on a phone (no board: a new screen in the Expo kit's look, as
//! the incoming screen is): a link to a call you were not asked into
//! opened. The caller's face and name in the middle with where you stand
//! (about to ask, waiting, turned away, unanswered), and at the foot, in
//! the thumb's reach, what you can do: Ask to join or Not now; Cancel
//! while you wait; Ask again or Back after. It stands in for the check
//! while you are at a door.
//!
//! Reads the door and the caller's name. Writes the knock, and the way
//! back to the check.
use view::design::{Kind as ButtonKind, Ladder, Nav};
use view::prelude::*;
use view::{Div, FontWeight};

use crate::call::AtTheDoor;
use crate::words;
use crate::{DoorStage, Huddle};

/// The caller's face.
const FACE: Pixels = px(72.);

pub(super) fn screen(
    view: &Huddle,
    door: &AtTheDoor,
    cx: &mut Context<Huddle>,
    ladder: &Ladder,
    theme: &Theme,
) -> impl IntoElement {
    let caller = view.name(door.caller);
    let (title, line) = words::at_the_door(door.stage, &caller);
    let asks = match door.stage {
        DoorStage::Asking => Some(words::ASK_TO_JOIN),
        DoorStage::Waiting => None,
        DoorStage::TurnedAway | DoorStage::NoAnswer => Some(words::ASK_AGAIN),
    };
    let leaves = match door.stage {
        DoorStage::Asking => words::NOT_NOW,
        DoorStage::Waiting => words::CANCEL,
        DoorStage::TurnedAway | DoorStage::NoAnswer => words::BACK,
    };
    let ask = cx.listener(|view: &mut Huddle, _: &ClickEvent, _, cx| view.ask_to_join(cx));
    let leave = cx.listener(|view: &mut Huddle, _: &ClickEvent, _, cx| view.leave_door(cx));
    design::page("huddle-door", words::HUDDLE, Nav::Child, ladder, theme)
        .child(
            div()
                .id("huddle-door-caller")
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(design::space::LG)
                .px(design::space::BLOCK)
                .text_center()
                .role(Role::Status)
                .aria_live(view::accesskit::Live::Polite)
                .child(design::avatar(&caller, FACE, theme))
                .child(
                    div()
                        .id("huddle-door-title")
                        .text_type(ladder.title)
                        .font_weight(FontWeight::MEDIUM)
                        .child(title),
                )
                .when(!line.is_empty(), |caller| {
                    caller.child(
                        div()
                            .text_type(ladder.secondary)
                            .text_color(theme.muted)
                            .child(line),
                    )
                })
                .children(view.call_note.clone().map(|note| {
                    div()
                        .id("huddle-call-note")
                        .text_type(ladder.caption)
                        .text_color(theme.warning)
                        .child(note)
                })),
        )
        .child(foot(asks, leaves, ask, leave, ladder, theme))
}

/// The foot: the ask (none while waiting) over the way back, each across
/// the width.
fn foot(
    asks: Option<&'static str>,
    leaves: &'static str,
    ask: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    leave: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ladder: &Ladder,
    theme: &Theme,
) -> Div {
    div()
        .flex_none()
        .flex()
        .flex_col()
        .gap(design::space::SM)
        .p(design::space::BLOCK)
        .border_t_1()
        .border_color(theme.border)
        .children(asks.map(|label| {
            design::button("huddle-ask", label, ladder, theme, ask).kind(ButtonKind::Primary)
        }))
        .child(design::button(
            "huddle-door-leave",
            leaves,
            ladder,
            theme,
            leave,
        ))
}
