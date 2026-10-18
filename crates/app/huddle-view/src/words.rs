//! What the view says, once: every sentence the phone's tree shows, and
//! each one the desktop's tree shows too (the trouble a piece reports, the
//! names of the toggles, what a device list and a picker say, the room's
//! words). A sentence only the desktop says stays in its tree.
use view::prelude::*;

use crate::{DoorStage, Kind, RECORD_SECONDS, Which};

/// Which piece a trouble line speaks of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Device {
    Camera,
    Microphone,
}

/// The view's name, and the window's title on the check.
pub(crate) const HUDDLE: &str = "Huddle";

/// The window's title in the room: just you, or how many are in it.
pub(crate) fn in_the_room(people: usize) -> String {
    match people {
        0 | 1 => "Huddle \u{b7} just you".into(),
        people => format!("Huddle \u{b7} {people}"),
    }
}

/// A picker naming no device the host listed: the system picks.
pub(crate) const SYSTEM_DEFAULT: &str = "System default";

/// A device list with nothing in it.
pub(crate) const NONE_FOUND: &str = "None found";

/// A device list the host has not answered yet.
pub(crate) const LISTING: &str = "Looking for devices\u{2026}";

/// Your tile's and your chip's name.
pub(crate) const YOU: &str = "You";

/// After your name while the picture is mirrored.
pub(crate) const MIRRORED: &str = "\u{b7} mirrored";

/// The marks a chip carries, as a reader names them.
pub(crate) const MUTED: &str = "Muted";
pub(crate) const SPEAKING: &str = "Speaking";

/// Under your face while the camera is off.
pub(crate) const CAMERA_IS_OFF: &str = "Your camera is off";

/// A camera slow to open.
pub(crate) const OPENING_CAMERA: &str = "Opening the camera\u{2026}";

/// The microphone's level, as a reader names the meter.
pub(crate) const MICROPHONE_LEVEL: &str = "Microphone level";

/// The speaker test, and the test while it sounds.
pub(crate) const TEST: &str = "Test";
pub(crate) const PLAYING: &str = "Playing\u{2026}";

/// Why the last speaker test did not sound, as the host said.
pub(crate) fn test_failed(why: &str) -> String {
    format!("Test failed: {why}.")
}

/// The live loop's control.
pub(crate) const HEAR_MYSELF: &str = "Hear myself live";

/// The mirror's control.
pub(crate) const MIRROR: &str = "Mirror my picture";

/// Into the room.
pub(crate) const JOIN: &str = "Join huddle";

/// Out of the room.
pub(crate) const LEAVE: &str = "Leave";

/// The room says when no one else is in it: Invite rings people, and a
/// call's link brings them to its door.
pub(crate) fn solo(call: bool) -> &'static str {
    match call {
        false => "Only you are here. Invite someone, or start a call and share its link.",
        true => "Only you are here. Invite someone, or copy the link.",
    }
}

/// What fixes a device's trouble.
pub(crate) const OPEN_SETTINGS: &str = "Open Settings";
pub(crate) const TRY_AGAIN: &str = "Try again";

/// A build without camera and microphone.
pub(crate) const UNSUPPORTED: &str = "No camera or microphone in this build";
pub(crate) const UNSUPPORTED_DETAIL: &str = "This copy of Ducktape was built without camera and \
     microphone support, so a huddle has nothing to carry. Install a full build.";

/// The room's clock, and who is in the room.
pub(crate) fn in_room(clock: &str, people: usize) -> String {
    match people {
        0 | 1 => format!("{clock} \u{b7} only you"),
        people => format!("{clock} \u{b7} {people}"),
    }
}

/// The count by the bar's people glyph, as a reader hears it.
pub(crate) fn in_the_huddle(people: usize) -> String {
    format!("{people} in the huddle")
}

/// The chip past the stage: the people not on it.
pub(crate) fn more(people: usize) -> String {
    format!("+{people}")
}

/// The same chip, as a reader hears it.
pub(crate) fn more_in_the_huddle(people: usize) -> String {
    format!("{people} more in the huddle")
}

// ---------- the call ----------

/// An account identity names nothing for.
pub(crate) fn account(number: u64) -> String {
    format!("account {number}")
}

/// A ring: the incoming card, the phone's incoming screen and the notice.
pub(crate) fn calling(caller: &str) -> String {
    format!("{caller} is calling")
}

/// Under a ring: how many are asked into the call, when it is more than
/// two (a call of two says nothing).
pub(crate) fn huddle_with(people: usize) -> String {
    match people {
        0..=2 => String::new(),
        people => format!("Huddle with {people} people"),
    }
}

/// A ring's answers.
pub(crate) const ANSWER: &str = "Answer";
pub(crate) const DECLINE: &str = "Decline";

/// The invite control and its list.
pub(crate) const INVITE: &str = "Invite";
pub(crate) const INVITE_PEOPLE: &str = "Invite people";

/// The invite list with no one in it.
pub(crate) const NO_ONE_TO_INVITE: &str = "No one else on this network yet";

/// Start a call with no one rung: its link is what brings people in.
pub(crate) const START_CALL: &str = "Start a call";

/// The call's link onto the clipboard, and the control's word once it is.
pub(crate) const COPY_LINK: &str = "Copy link";
pub(crate) const LINK_COPIED: &str = "Link copied";

// ---------- the door ----------

/// A knock, as the room and the notice say it.
pub(crate) fn asking_to_join(who: &str) -> String {
    format!("{who} is asking to join")
}

/// The notice to one let in at `caller`'s call's door.
pub(crate) fn let_into(caller: &str) -> String {
    format!("You were let into {caller}'s call")
}

/// A knock's answers.
pub(crate) const LET_IN: &str = "Let in";
pub(crate) const TURN_AWAY: &str = "Turn away";

/// What the knocker's screen says at each stage: a title and the line
/// under it.
pub(crate) fn at_the_door(stage: DoorStage, caller: &str) -> (String, &'static str) {
    match stage {
        DoorStage::Asking => (
            format!("{caller} is in a call"),
            "You were not asked in. Ask to join, and someone in the call can let you in.",
        ),
        DoorStage::Waiting => (
            "Waiting for someone to let you in\u{2026}".into(),
            "The people in the call see you asking.",
        ),
        DoorStage::TurnedAway => ("You were not let in.".into(), ""),
        DoorStage::NoAnswer => ("No one answered.".into(), "Ask again, or come back later."),
    }
}

/// The knocker's controls: the ask, its cancel, asking again, and back to
/// the check.
pub(crate) const ASK_TO_JOIN: &str = "Ask to join";
pub(crate) const NOT_NOW: &str = "Not now";
pub(crate) const CANCEL: &str = "Cancel";
pub(crate) const ASK_AGAIN: &str = "Ask again";
pub(crate) const BACK: &str = "Back";

/// A link to a call there is none of.
pub(crate) const NO_SUCH_CALL: &str = "There is no such call.";

/// The network row: the call's tracks are not through.
pub(crate) const RECONNECTING: &str = "Reconnecting\u{2026}";

/// A call refused by the program or the node, said in the room.
pub(crate) fn call_trouble(why: &str) -> String {
    format!("The call did not go through: {why}")
}

/// The call ended under the person: the caller ended it, or it was over.
pub(crate) const CALL_ENDED: &str = "The call ended.";

/// Why an op of the call is not known to have gone through: the node
/// never said.
pub(crate) const NO_ANSWER: &str = "no answer came back";

/// A kind of device, as its picker's label and its group in a menu say it.
pub(crate) fn kind(kind: Kind) -> &'static str {
    match kind {
        Kind::Camera => "Camera",
        Kind::Microphone => "Microphone",
        Kind::Speaker => "Speaker",
        Kind::Person => "People",
    }
}

/// A device menu's name.
pub(crate) fn menu(which: Which) -> &'static str {
    match which {
        Which::Kind(of) => kind(of),
        Which::Sound => "Microphone and speaker",
        Which::Routes => PLAY_THROUGH,
        Which::Invite => INVITE_PEOPLE,
    }
}

/// The microphone toggle over the check's picture, which names the device.
pub(crate) fn mute_microphone(muted: bool) -> &'static str {
    match muted {
        true => "Unmute microphone",
        false => "Mute microphone",
    }
}

/// The microphone toggle in the room and the float.
pub(crate) fn mute(muted: bool) -> &'static str {
    match muted {
        true => "Unmute",
        false => "Mute",
    }
}

/// The camera toggle.
pub(crate) fn camera_toggle(off: bool) -> &'static str {
    match off {
        true => "Turn camera on",
        false => "Turn camera off",
    }
}

/// Record and play back, as its control says it: to start, or to stop
/// the recording or the playback under way.
pub(crate) fn record(playback: &Playback) -> String {
    match playback {
        Playback::Idle => format!("Record {RECORD_SECONDS} s and play back"),
        Playback::Recording => "Stop".into(),
        Playback::Playing => "Stop playback".into(),
    }
}

/// The line under Record: what waits for the microphone, what the
/// recording does, or what it is for. A recording's countdown follows
/// [`RECORDING`].
pub(crate) fn record_line(muted: bool, playback: &Playback) -> &'static str {
    match playback {
        _ if muted => "Unmute to test your microphone.",
        Playback::Recording => RECORDING,
        Playback::Playing => "Playing it back\u{2026}",
        Playback::Idle => "Hear how you sound to others.",
    }
}

/// A recording under way, before its countdown.
pub(crate) const RECORDING: &str = "Recording\u{2026} say something \u{b7}\u{a0}";

/// Joining with a device that is missing.
pub(crate) fn join_without(device: Device) -> &'static str {
    match device {
        Device::Camera => "Join without camera",
        Device::Microphone => "Join without a microphone",
    }
}

// ---------- the phone's own ----------

/// The check's title row, under the view's name.
pub(crate) const CHECK_CAPTION: &str = "Check your camera and microphone";

/// The check's sections.
pub(crate) const SOUND: &str = "Sound";
pub(crate) const PICTURE: &str = "Picture";

/// The sound sheet's head, and its foot.
pub(crate) const PLAY_THROUGH: &str = "Play sound through";
pub(crate) const MICROPHONE_FOLLOWS: &str =
    "The microphone follows: AirPods or a headset bring their own.";

/// The room's sound control, named with the route.
pub(crate) fn sound(route: &str) -> String {
    format!("{SOUND}: {route}")
}

/// Under "Hear myself live", by where the sound goes: through a
/// loudspeaker (or a route not listed yet) the phone refuses the loop and
/// says why.
pub(crate) fn hear_myself_line(route: Option<AudioKind>) -> &'static str {
    match route {
        Some(AudioKind::Headphones) => {
            "Headphones connected: hear your own microphone as you speak."
        }
        Some(AudioKind::Earpiece | AudioKind::Other) => "Hear your own microphone as you speak.",
        Some(AudioKind::Speaker) | None => {
            "Through the speaker your voice loops back into the microphone and howls. Connect \
             headphones to hear yourself."
        }
    }
}

/// Under "Mirror my picture".
pub(crate) const MIRROR_LINE: &str = "Front camera only; the back camera is never mirrored.";

/// The flip, by the camera it switches to.
pub(crate) fn flip(to: Facing) -> &'static str {
    match to {
        Facing::Back => "Switch to the back camera",
        _ => "Switch to the front camera",
    }
}

/// While the system asks whether the camera may be used.
pub(crate) const ALLOW_CAMERA: &str = "Allow the camera when your phone asks.";

// ---------- troubles ----------

/// What a trouble is called where both designs call it the same: a
/// refused, busy or failed device.
fn called(device: Device, status: &MediaStatus) -> &'static str {
    let camera = device == Device::Camera;
    let either = |of_camera, of_microphone| if camera { of_camera } else { of_microphone };
    match status {
        MediaStatus::Denied => either("Camera access is off", "Microphone access is off"),
        MediaStatus::Busy => either(
            "Another app is using the camera",
            "Another app is using the microphone",
        ),
        _ => either("The camera did not start", "The microphone did not start"),
    }
}

/// The host's words for the trouble (`detail`), else `fallback`.
fn detail_or(state: &MediaState, fallback: &str) -> String {
    match state.detail.is_empty() {
        true => fallback.to_owned(),
        false => state.detail.clone(),
    }
}

/// The title and the line under it for a device's trouble on the desktop:
/// the platform's fix line, or why the device went away, is the host's
/// (`detail`); with none, the Linux one.
pub(crate) fn trouble(device: Device, state: &MediaState) -> (&'static str, String) {
    let camera = device == Device::Camera;
    let noun = if camera { "camera" } else { "microphone" };
    match state.status {
        MediaStatus::Denied => {
            let fix = detail_or(
                state,
                &format!(
                    "Check the device's permissions (or your desktop's {}), then try again.",
                    if camera {
                        "camera portal"
                    } else {
                        "sound settings"
                    }
                ),
            );
            let why = match state.opens_settings {
                true => format!("Ducktape is not allowed to use the {noun}."),
                false => format!("Your system refused this app the {noun}."),
            };
            (called(device, &state.status), format!("{why} {fix}"))
        }
        MediaStatus::NoDevice => (
            if camera {
                "No camera found"
            } else {
                "No microphone found"
            },
            match camera {
                true => "Plug one in, or join with your microphone only.".into(),
                false => "Plug one in, or join with your camera only.".into(),
            },
        ),
        MediaStatus::Gone => (
            if camera {
                "The camera went away"
            } else {
                "The microphone went away"
            },
            detail_or(
                state,
                match camera {
                    true => {
                        "It was unplugged or its driver stopped. The last picture is gone; \
                         plug it back in and try again."
                    }
                    false => {
                        "It was unplugged or its driver stopped; plug it back in and try again."
                    }
                },
            ),
        ),
        MediaStatus::Busy => (
            called(device, &state.status),
            "Close it (a browser tab, Zoom, OBS), then try again.".into(),
        ),
        MediaStatus::Failed | MediaStatus::Unsupported => {
            (called(device, &state.status), detail_or(state, ""))
        }
        MediaStatus::Starting | MediaStatus::Live => ("", String::new()),
    }
}

/// The title and the line under it for a device's trouble on a phone,
/// where a call or another app takes a device over and Settings is the
/// one way back to a refused one: a camera's card says both; a
/// microphone's trouble is one line, [`phone_line`].
pub(crate) fn phone_trouble(device: Device, state: &MediaState) -> (&'static str, String) {
    let camera = device == Device::Camera;
    let either = |of_camera: &'static str, of_microphone: &'static str| match camera {
        true => of_camera,
        false => of_microphone,
    };
    match state.status {
        MediaStatus::Denied => (
            called(device, &state.status),
            either(
                "Ducktape is not allowed to use the camera. Allow it in Settings, then try again.",
                "Allow it in Settings, then try again.",
            )
            .into(),
        ),
        MediaStatus::Busy => (
            called(device, &state.status),
            either("Close it, then try again.", "").into(),
        ),
        MediaStatus::Gone => (
            either("The camera stopped", "The microphone stopped"),
            detail_or(
                state,
                either(
                    "A phone call or another app took it over. When that is done, try again.",
                    "",
                ),
            ),
        ),
        MediaStatus::NoDevice => (
            either("No camera on this device", "No microphone on this device"),
            either("Join with your microphone only.", "").into(),
        ),
        MediaStatus::Failed | MediaStatus::Unsupported => {
            (called(device, &state.status), detail_or(state, ""))
        }
        MediaStatus::Starting | MediaStatus::Live => ("", String::new()),
    }
}

/// A device's trouble on a phone as one line: its title and what follows,
/// each a sentence.
pub(crate) fn phone_line(device: Device, state: &MediaState) -> String {
    match phone_trouble(device, state) {
        (title, text) if text.is_empty() => format!("{title}."),
        (title, text) => format!("{title}. {text}"),
    }
}
