//! The door on the desktop: a link to a call you were not asked into
//! opened. In the middle of the window, the caller's face and name, where
//! you stand (about to ask, waiting, turned away, unanswered) and what you
//! can do: Ask to join or Not now; Cancel while you wait; Ask again or Back
//! after. Why a knock did not go through is said under them.
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use crate::call::AtTheDoor;
use crate::words;
use crate::{DoorStage, Huddle};

/// The caller's face.
const FACE: Pixels = px(56.);

/// What takes the keys when the door opens.
pub(super) const ASK: &str = "huddle-ask";

pub(super) fn screen(
    view: &Huddle,
    door: &AtTheDoor,
    cx: &mut Context<Huddle>,
    theme: &Theme,
) -> Stateful<Div> {
    let caller = view.name(door.caller);
    let (title, line) = words::at_the_door(door.stage, &caller);
    let ask = cx.listener(|view: &mut Huddle, _: &ClickEvent, _, cx| view.ask_to_join(cx));
    let leave = cx.listener(|view: &mut Huddle, _: &ClickEvent, window, cx| {
        view.leave_door(cx);
        window.focus(super::JOIN);
    });
    let pointer = &design::POINTER;
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
    div()
        .id("huddle-door")
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(design::space::SM)
        .p(design::space::BLOCK)
        .text_center()
        .role(Role::Status)
        .aria_live(view::accesskit::Live::Polite)
        .child(design::avatar(&caller, FACE, theme))
        .child(
            div()
                .id("huddle-door-title")
                .text_size(design::text::BODY)
                .font_weight(FontWeight::MEDIUM)
                .child(title),
        )
        .when(!line.is_empty(), |door| {
            door.child(
                div()
                    .max_w(px(320.))
                    .text_size(design::text::CAPTION)
                    .text_color(theme.muted)
                    .child(line),
            )
        })
        .children(view.call_note.clone().map(|note| {
            div()
                .id("huddle-call-note")
                .max_w(px(320.))
                .text_size(design::text::CAPTION)
                .text_color(theme.warning)
                .child(note)
        }))
        .child(
            div()
                .mt(design::space::SM)
                .flex()
                .gap(design::space::SM)
                .children(asks.map(|label| {
                    design::button(ASK, label, pointer, theme, ask).kind(design::Kind::Primary)
                }))
                .child(
                    design::button("huddle-door-leave", leaves, pointer, theme, leave)
                        .kind(design::Kind::Outline),
                ),
        )
}
