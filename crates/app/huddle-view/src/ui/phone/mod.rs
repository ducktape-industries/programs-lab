//! The phone's Huddle, the view's second design (`wt/briefs/huddle/
//! design-phone/SPEC.md`, boards 10 to 30; Expo has no huddle screen, so
//! these are new screens in the Expo kit's look, `design::TOUCH`). The
//! view's root picks it once, when the window's pointer is a finger;
//! nothing under here asks again.
//!
//! A phone keeps one full-screen window: Join keeps the view where it is
//! and nothing here floats it. The phone's route sets its microphone and
//! speaker together, so one Sound row and its sheet stand for both menus;
//! its front and back cameras make a flip, not a menu. A call ringing you
//! takes the whole screen.
//!
//! It reads the model (`lib.rs`) and the words both designs show
//! (`words.rs`), and writes only through the model's verbs; it never
//! reaches into the desktop's `ui/` files. One file per screen:
//! - `check.rs`: the device check;
//! - `incoming.rs`: a call ringing you;
//! - `door.rs`: a call you were not asked into, whose link you opened;
//! - `room.rs`: the room;
//! - `parts.rs`: what the check and the room both draw.
//!
//! This file draws the screen up, the sheet over it (the sound routes, or
//! the people to invite), and the screen of a build without camera and
//! microphone.
use view::design::{Ladder, Nav};
use view::prelude::*;
use view::{AnyElement, Div, Stateful};

use crate::look::{CHECK, HEADPHONES, SPEAKER, VIDEO_OFF};
use crate::words;
use crate::{Huddle, Row, Screen, Which};

mod check;
mod door;
mod incoming;
mod parts;
mod room;

/// The sound sheet is the routes' menu: on a phone the speaker picked is
/// the route, which the microphone follows.
const SOUND: Which = Which::Routes;

/// A person in the invite sheet, by the Lucide name `design::icon` draws.
const PERSON: &str = "user-round";

/// The screen, and the sheet over it. Back closes that sheet, and is there
/// exactly while one is drawn: on the check the way out is the host's
/// capsule, and the room ends by Leave alone.
pub(crate) fn render(view: &Huddle, cx: &mut Context<Huddle>) -> AnyElement {
    let theme = *cx.global::<Theme>();
    let ladder = &design::TOUCH;
    let supported = !view.no_support();
    // what is drawn, decided once: the screen and the sheet over it
    let sheet = supported.then(|| sheet(view, cx, ladder, &theme)).flatten();
    let ring = view.rings().first().map(|ring| (*ring).clone());
    let screen = match (supported, view.screen, ring, &view.door) {
        (false, ..) => unsupported(ladder, &theme).into_any_element(),
        (true, Screen::Check, Some(ring), _) => {
            incoming::screen(view, &ring, cx, ladder, &theme).into_any_element()
        }
        (true, Screen::Check, None, Some(door)) => {
            door::screen(view, door, cx, ladder, &theme).into_any_element()
        }
        (true, Screen::Check, None, None) => {
            check::screen(view, cx, ladder, &theme).into_any_element()
        }
        (true, Screen::Room, ..) => room::screen(view, cx, ladder, &theme).into_any_element(),
    };
    div()
        .id("huddle")
        .size_full()
        .flex()
        .flex_col()
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_type(ladder.body)
        .when(sheet.is_some(), |root| {
            root.on_back(cx.listener(|view: &mut Huddle, _: &BackEvent, _, cx| {
                view.close_menu(cx);
            }))
        })
        .child(screen)
        .children(sheet)
        .into_any_element()
}

/// The sheet open, if one is: the sound sheet (SPEC §1.2), the routes the
/// host lists, the one in use checked, a pick playing through it; or the
/// people to invite, a pick ringing them. A pick closes it.
fn sheet(
    view: &Huddle,
    cx: &mut Context<Huddle>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<AnyElement> {
    let menu = view.menu?;
    let (id, foot) = match menu.which {
        SOUND => ("huddle-sound", Some(words::MICROPHONE_FOLLOWS)),
        Which::Invite => ("huddle-invite", None),
        _ => return None,
    };
    let close = cx.listener(|view: &mut Huddle, _: &(), _, cx| {
        view.close_menu(cx);
    });
    let rows = view
        .rows(menu.which)
        .into_iter()
        .enumerate()
        .map(|(index, row)| match menu.which {
            SOUND => route(view, index, row, cx, ladder, theme),
            _ => person(index, row, cx, ladder, theme),
        })
        .collect::<Vec<_>>();
    let name = words::menu(menu.which);
    let sheet = div()
        .flex()
        .flex_col()
        .child(design::sheet_title(
            format!("{id}-sheet-head"),
            name,
            ladder,
            theme,
        ))
        .child(
            div()
                .id(SharedString::from(format!("{id}-rows")))
                .role(Role::Menu)
                .aria_label(name)
                .children(rows),
        )
        .children(foot.map(|foot| {
            div()
                .px(design::SHEET_INSET)
                .py(design::space::LG)
                .text_type(ladder.secondary)
                .text_color(theme.muted)
                .child(foot)
        }));
    Some(
        design::sheet(format!("{id}-sheet"), name, ladder, theme)
            .on_dismiss(close)
            .child(sheet)
            .into_any_element(),
    )
}

/// Row `index` of the invite sheet: a person, rung on a press; or, with
/// no one listed, why.
fn person(
    index: usize,
    row: Row,
    cx: &mut Context<Huddle>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    if row.id.is_none() {
        return nothing(row.name, ladder, theme).into_any_element();
    }
    let ring = cx.listener(move |view: &mut Huddle, _: &ClickEvent, _, cx| {
        view.pick(index, cx);
    });
    design::sheet_row(
        format!("huddle-invite-{index}"),
        row.name,
        ladder,
        theme,
        ring,
    )
    .icon(PERSON)
    .into_element()
    .role(Role::MenuItem)
    .into_any_element()
}

/// Row `index` of the sound sheet: the route's glyph by what it is, its
/// name, and a check on the one in use; or, with none listed, why.
fn route(
    view: &Huddle,
    index: usize,
    row: Row,
    cx: &mut Context<Huddle>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let Some(id) = row.id else {
        return nothing(row.name, ladder, theme).into_any_element();
    };
    let glyph = match view.speaker(&id).map(|speaker| speaker.kind) {
        Some(AudioKind::Headphones) => HEADPHONES,
        Some(AudioKind::Earpiece) => parts::SMARTPHONE,
        _ => SPEAKER,
    };
    let pick = cx.listener(move |view: &mut Huddle, _: &ClickEvent, _, cx| {
        view.pick(index, cx);
    });
    design::sheet_row(
        format!("huddle-sound-{index}"),
        row.name,
        ladder,
        theme,
        pick,
    )
    .icon(glyph)
    .when_some(row.twin, |row, twin| row.hint(twin))
    .into_element()
    // one of the routes, as the desktop's menu rows are
    .role(Role::MenuItemRadio)
    .aria_toggled(row.picked.into())
    .when(row.picked, |route| {
        route.child(design::icon(CHECK, ladder.icon_sheet).text_color(theme.foreground))
    })
    .into_any_element()
}

/// A sheet's one line where the host lists no route, or identity no one:
/// still asking, its refusal, or none found.
fn nothing(why: String, ladder: &Ladder, theme: &Theme) -> Stateful<Div> {
    div()
        .id("huddle-sheet-none")
        .h(ladder.sheet_row)
        .px(design::SHEET_INSET)
        .flex()
        .items_center()
        .text_type(ladder.body)
        .text_color(theme.muted)
        .child(why)
}

/// A build without camera and microphone: said once, under the title.
fn unsupported(ladder: &Ladder, theme: &Theme) -> impl IntoElement {
    design::page(
        "huddle-unsupported-page",
        words::HUDDLE,
        Nav::Child,
        ladder,
        theme,
    )
    .child(
        div()
            .flex_1()
            .flex()
            .flex_col()
            .justify_center()
            .child(design::empty_state(
                "huddle-unsupported",
                VIDEO_OFF,
                words::UNSUPPORTED,
                words::UNSUPPORTED_DETAIL,
                ladder,
                theme,
            )),
    )
}
