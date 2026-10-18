//! The phone's design (`ui/phone/`), as the phone boards draw it: each
//! state of the check and of the room, and each action, under a finger at
//! 390 wide. The sweep holds them all at 360, 390 and 430.
use super::*;

/// A phone's host: a front and a back camera, its microphone, and three
/// routes (the loudspeaker, a huddle's default; AirPods; the earpiece).
/// It would float a pane if asked: the phone's tree never asks.
fn phone_host() -> (TestAppContext, StreamSender<ClockTicks>) {
    phone_routes(vec![
        audio("speaker", "Speaker", true, AudioKind::Speaker),
        audio("airpods", "AirPods Pro", false, AudioKind::Headphones),
        audio(
            "earpiece",
            "Phone (hold to your ear)",
            false,
            AudioKind::Earpiece,
        ),
    ])
}

/// A phone whose host lists `outputs` as its routes.
fn phone_routes(outputs: Vec<AudioDevice>) -> (TestAppContext, StreamSender<ClockTicks>) {
    let cx = TestAppContext::new();
    cx.host().handle::<CameraDevices>(|()| {
        Ok(vec![
            camera("front", "Front Camera", true, Facing::Front),
            camera("back", "Back Camera", false, Facing::Back),
        ])
    });
    cx.host().handle::<MicrophoneDevices>(move |()| {
        Ok(AudioDevices {
            inputs: vec![audio("mic", "Phone microphone", true, AudioKind::Other)],
            outputs: outputs.clone(),
        })
    });
    cx.host().handle::<HostFloat>(|float| {
        Ok(Placement {
            floating: true,
            on_top: float.on_top,
        })
    });
    let ticks = cx.host().stream::<ClockTicks>();
    (cx, ticks)
}

/// The view open on [`phone_host`], under a finger, 390 by 844.
fn phone() -> (TestAppContext, StreamSender<ClockTicks>) {
    opened_on_phone(phone_host())
}

/// The view open on `host`, under a finger, 390 by 844.
fn opened_on_phone(
    (mut cx, ticks): (TestAppContext, StreamSender<ClockTicks>),
) -> (TestAppContext, StreamSender<ClockTicks>) {
    cx.open::<Huddle>();
    cx.run_until_parked();
    on_phone(&mut cx);
    (cx, ticks)
}

fn on_phone(cx: &mut TestAppContext) {
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
}

/// Both pieces live: the camera's picture drawn, the microphone's level
/// moving.
fn live_pieces(cx: &mut TestAppContext) {
    let (camera, ..) = camera_piece(cx).expect("a camera piece");
    cx.simulate_media_state(camera.clone(), live("Front Camera"));
    let mic = mic_key(cx);
    cx.simulate_media_state(mic.clone(), live("Phone microphone"));
}

/// The route row `name` picked in the sound sheet.
fn route(cx: &mut TestAppContext, name: &str) {
    cx.simulate_click("huddle-sound");
    cx.run_until_parked();
    cx.simulate_click(name);
}

fn has_all(cx: &TestAppContext, texts: &[&str]) {
    for text in texts {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
}

/// The sweep: every board's state, every frame audited and fitted at each
/// phone width.
#[test]
fn sweep() {
    let (mut cx, ticks) = phone();
    view::testing::sweep(&mut cx, |cx| {
        // board 10: the check, live; board 12: the sheet, AirPods and the
        // live loop through them, then the speaker again
        live_pieces(cx);
        cx.simulate_click("huddle-sound");
        cx.simulate_click("huddle-sound-1");
        let mic = mic_key(cx);
        cx.simulate_media_state(mic.clone(), live("AirPods Pro"));
        cx.simulate_click("huddle-monitor");
        route(cx, "huddle-sound-0");
        // recording, playing it back
        let mic = mic_key(cx);
        for playback in [Playback::Recording, Playback::Playing, Playback::Idle] {
            cx.simulate_media_state(
                mic.clone(),
                MediaState {
                    playback,
                    ..live("Phone microphone")
                },
            );
        }
        // board 11: the camera off and muted; the back camera
        cx.simulate_click("huddle-camera");
        cx.simulate_click("huddle-mic");
        cx.simulate_click("huddle-camera");
        cx.simulate_click("huddle-mic");
        cx.simulate_click("huddle-flip");
        cx.simulate_click("huddle-flip");
        // boards 20 and 21: the camera asking, then each trouble
        ticks.send(());
        ticks.send(());
        cx.run_until_parked();
        for status in [
            MediaStatus::Denied,
            MediaStatus::Busy,
            MediaStatus::Gone,
            MediaStatus::NoDevice,
            MediaStatus::Failed,
        ] {
            let (camera, ..) = camera_piece(cx).expect("a camera piece");
            cx.simulate_media_state(
                camera.clone(),
                MediaState {
                    status,
                    opens_settings: true,
                    ..Default::default()
                },
            );
            cx.simulate_click("huddle-camera-retry");
            cx.run_until_parked();
        }
        // board 21: the microphone refused (Open Settings and Try again
        // under the route), then missing (Try again, join without it)
        for status in [MediaStatus::Denied, MediaStatus::NoDevice] {
            let mic = mic_key(cx);
            cx.simulate_media_state(
                mic.clone(),
                MediaState {
                    status,
                    opens_settings: true,
                    ..Default::default()
                },
            );
            cx.simulate_click("huddle-microphone-retry");
            cx.run_until_parked();
        }
        // board 30: the room, speaking, muted with the camera off, the
        // microphone in trouble, the sheet over it
        live_pieces(cx);
        cx.simulate_click("huddle-join");
        let mic = mic_key(cx);
        cx.simulate_media_state(
            mic.clone(),
            MediaState {
                speaking: true,
                ..live("Phone microphone")
            },
        );
        cx.simulate_click("huddle-camera");
        cx.simulate_click("huddle-mic");
        cx.simulate_click("huddle-camera");
        cx.simulate_click("huddle-mic");
        // the camera in trouble in the room: its card over the tile
        for status in [MediaStatus::Denied, MediaStatus::NoDevice] {
            let (camera, ..) = camera_piece(cx).expect("a camera piece");
            cx.simulate_media_state(
                camera.clone(),
                MediaState {
                    status,
                    opens_settings: true,
                    ..Default::default()
                },
            );
            cx.simulate_click("huddle-camera-retry");
            cx.run_until_parked();
        }
        let mic = mic_key(cx);
        cx.simulate_media_state(
            mic.clone(),
            MediaState {
                status: MediaStatus::Denied,
                opens_settings: true,
                ..Default::default()
            },
        );
        cx.simulate_click("huddle-microphone-retry");
        cx.simulate_click("huddle-sound");
        cx.simulate_back();
        cx.simulate_click("huddle-leave");
    });
}

/// A build without camera and microphone, at each phone width: one empty
/// state under the title, no Join.
#[test]
fn a_build_without_media_says_so() {
    let mut cx = TestAppContext::new();
    let unknown = view::methods::refusal::UNKNOWN_REQUEST;
    cx.host().refuse::<CameraDevices>(unknown, "no camera here");
    cx.host()
        .refuse::<MicrophoneDevices>(unknown, "no microphone here");
    let _ticks = cx.host().stream::<ClockTicks>();
    cx.open::<Huddle>();
    cx.run_until_parked();
    view::testing::sweep(&mut cx, |cx| {
        assert!(cx.find("huddle-unsupported").is_some());
        assert!(cx.find("huddle-join").is_none());
    });
    has_all(
        &cx,
        &[
            "No camera or microphone in this build",
            "This copy of Ducktape was built without camera and microphone support, so a huddle \
             has nothing to carry. Install a full build.",
        ],
    );
}

/// The pointer picks the design, not the width: a finger gets the phone's
/// check on a wide window, a mouse the desktop's on a narrow one.
#[test]
fn the_pointer_picks_the_design_not_the_width() {
    let (mut cx, _ticks) = phone();
    cx.simulate_resize(820., 800.);
    assert!(cx.find("huddle-sound-row").is_some());
    assert!(cx.find("huddle-pick-camera").is_none());
    cx.simulate_touch(false);
    cx.simulate_resize(360., 800.);
    assert!(cx.find("huddle-sound-row").is_none());
    assert!(cx.find("huddle-pick-camera").is_some());
}

/// Opened: the front camera mirrored and covering its square, the
/// microphone naming no device (it follows the route), the Sound row
/// naming the route, and the words of board 10.
#[test]
fn the_check_opens_on_the_front_camera_and_the_speaker() {
    let (cx, _ticks) = phone();
    let (_, device, mirror, fit) = camera_piece(&cx).expect("a camera piece");
    assert_eq!((device, mirror, fit), (None, true, ImageObjectFit::Cover));
    let (_, input, output, monitor, _) = mic_piece(&cx).expect("a microphone piece");
    assert_eq!((input, output, monitor), (None, None, false));
    assert_eq!(
        cx.interactivity("huddle-sound").aria.label.as_deref(),
        Some("Sound: Speaker")
    );
    has_all(
        &cx,
        &[
            "Huddle",
            "Check your camera and microphone",
            "You",
            "\u{b7} mirrored",
            "Sound",
            "Speaker",
            "Test",
            "Record 5 s and play back",
            "Hear how you sound to others.",
            "Hear myself live",
            "Picture",
            "Mirror my picture",
            "Front camera only; the back camera is never mirrored.",
            "Join huddle",
        ],
    );
    assert_eq!(
        cx.interactivity("huddle-flip").aria.label.as_deref(),
        Some("Switch to the back camera")
    );
}

/// The flip turns to the camera on the other side and back; the back
/// camera is never mirrored, and the mirror stands still while it is in
/// use.
#[test]
fn the_flip_turns_to_the_other_side_and_the_back_is_never_mirrored() {
    let (mut cx, _ticks) = phone();
    cx.simulate_click("huddle-flip");
    let (_, device, mirror, _) = camera_piece(&cx).unwrap();
    assert_eq!((device.as_deref(), mirror), (Some("back"), false));
    assert!(!cx.has_text("\u{b7} mirrored"));
    assert!(disabled(&cx, "huddle-mirror"), "the back is never mirrored");
    assert_eq!(
        cx.interactivity("huddle-flip").aria.label.as_deref(),
        Some("Switch to the front camera")
    );
    cx.simulate_click("huddle-flip");
    let (_, device, mirror, _) = camera_piece(&cx).unwrap();
    assert_eq!((device.as_deref(), mirror), (Some("front"), true));
    assert!(!disabled(&cx, "huddle-mirror"));
}

/// No flip where the host lists no camera on the other side, and none
/// while the camera is off (it stands still) or in trouble (it goes).
#[test]
fn the_flip_shows_only_with_a_front_and_a_back_camera() {
    let (mut cx, _ticks) = phone_host();
    cx.host().handle::<CameraDevices>(|()| {
        Ok(vec![
            camera("front", "Front Camera", true, Facing::Front),
            camera("usb", "USB Camera", false, Facing::External),
        ])
    });
    cx.open::<Huddle>();
    cx.run_until_parked();
    on_phone(&mut cx);
    assert!(cx.find("huddle-flip").is_none());

    let (mut cx, _ticks) = phone();
    cx.simulate_click("huddle-camera");
    assert!(disabled(&cx, "huddle-flip"), "the camera is off");
    cx.simulate_click("huddle-camera");
    let (camera, ..) = camera_piece(&cx).unwrap();
    cx.simulate_media_state(camera.clone(), status(MediaStatus::Busy));
    assert!(
        cx.find("huddle-flip").is_none(),
        "a card says what is wrong"
    );
}

/// The sound sheet lists the routes as the host does, the speaker in use
/// checked; a pick plays through it, closes the sheet, and the Sound row
/// names it.
#[test]
fn the_sound_sheet_lists_the_routes_and_picks_one() {
    let (mut cx, _ticks) = phone();
    cx.simulate_click("huddle-sound");
    has_all(
        &cx,
        &[
            "Play sound through",
            "AirPods Pro",
            "Phone (hold to your ear)",
            "The microphone follows: AirPods or a headset bring their own.",
        ],
    );
    assert_eq!(toggled(&cx, "huddle-sound-0"), Some(true));
    assert_eq!(toggled(&cx, "huddle-sound-1"), Some(false));
    assert_eq!(
        cx.interactivity("huddle-sound-0").role,
        Some(Role::MenuItemRadio)
    );
    cx.simulate_click("huddle-sound-1");
    assert!(cx.find("huddle-sound-sheet").is_none(), "closed");
    let (_, input, output, ..) = mic_piece(&cx).unwrap();
    assert_eq!((input, output.as_deref()), (None, Some("airpods")));
    assert_eq!(
        cx.interactivity("huddle-sound").aria.label.as_deref(),
        Some("Sound: AirPods Pro")
    );
}

/// The iPhone's host lists the loudspeaker, the route playing now and each
/// Bluetooth headset, never the earpiece: the sheet reads with the
/// loudspeaker alone (the simulator), and with AirPods playing it names
/// them in use, where the host lists them, and lets the live loop run
/// through them.
#[test]
fn the_sound_sheet_reads_with_the_iphones_routes() {
    let (mut cx, _ticks) = opened_on_phone(phone_routes(vec![speaker(true)]));
    live_pieces(&mut cx);
    assert_eq!(
        cx.interactivity("huddle-sound").aria.label.as_deref(),
        Some("Sound: Speaker")
    );
    cx.simulate_click("huddle-sound");
    assert_eq!(toggled(&cx, "huddle-sound-0"), Some(true));
    assert!(cx.find("huddle-sound-1").is_none(), "one route");

    let (mut cx, _ticks) = opened_on_phone(phone_routes(vec![speaker(false), airpods(true)]));
    live_pieces(&mut cx);
    assert_eq!(
        cx.interactivity("huddle-sound").aria.label.as_deref(),
        Some("Sound: AirPods Pro")
    );
    assert!(!disabled(&cx, "huddle-monitor"), "through the AirPods");
    cx.simulate_click("huddle-sound");
    has_all(&cx, &["Speaker", "AirPods Pro"]);
    assert_eq!(toggled(&cx, "huddle-sound-0"), Some(false));
    assert_eq!(toggled(&cx, "huddle-sound-1"), Some(true), "in use");
}

/// The routes stay as the host lists them: on an iPhone the route playing
/// is the default, and it moves to the one picked; listed first, the rows
/// would jump under the finger.
#[test]
fn the_routes_keep_their_places_when_the_route_playing_moves() {
    let (mut cx, _ticks) = opened_on_phone(phone_routes(vec![speaker(true), airpods(false)]));
    route(&mut cx, "huddle-sound-1");
    // the phone plays through the AirPods now, and lists them so
    cx.host().handle::<MicrophoneDevices>(|()| {
        Ok(AudioDevices {
            inputs: vec![audio("mic", "Phone microphone", true, AudioKind::Other)],
            outputs: vec![speaker(false), airpods(true)],
        })
    });
    cx.simulate_click("huddle-sound");
    cx.run_until_parked();
    assert_eq!(toggled(&cx, "huddle-sound-0"), Some(false), "the speaker");
    assert_eq!(toggled(&cx, "huddle-sound-1"), Some(true), "the AirPods");
}

/// The live loop never runs through the loudspeaker, picked or not: a
/// list read again whose route playing moved from the AirPods to the
/// loudspeaker (they were taken off) stops the loop at once, though the
/// switch was on and nothing was picked.
#[test]
fn the_live_loop_stops_when_the_route_playing_moves_to_the_speaker() {
    let (mut cx, _ticks) = opened_on_phone(phone_routes(vec![speaker(false), airpods(true)]));
    live_pieces(&mut cx);
    cx.simulate_click("huddle-monitor");
    assert!(mic_piece(&cx).unwrap().3, "looping through the AirPods");
    cx.host().handle::<MicrophoneDevices>(|()| {
        Ok(AudioDevices {
            inputs: vec![audio("mic", "Phone microphone", true, AudioKind::Other)],
            outputs: vec![speaker(true), airpods(false)],
        })
    });
    // opening the sheet reads the lists again
    cx.simulate_click("huddle-sound");
    cx.run_until_parked();
    cx.simulate_back();
    assert!(!mic_piece(&cx).unwrap().3, "never through the loudspeaker");
}

/// The host stopped the live loop before the phone read its routes again
/// (the AirPods were taken off): the switch turns off and stays off.
#[test]
fn a_live_loop_the_host_stopped_turns_its_switch_off() {
    let (mut cx, _ticks) = opened_on_phone(phone_routes(vec![speaker(false), airpods(true)]));
    live_pieces(&mut cx);
    cx.simulate_click("huddle-monitor");
    assert!(mic_piece(&cx).unwrap().3, "looping through the AirPods");
    let mic = mic_key(&cx);
    let stopped = MediaState {
        monitor_stopped: true,
        ..live("Phone microphone")
    };
    cx.simulate_media_state(mic.clone(), stopped);
    assert!(!mic_piece(&cx).unwrap().3, "the loop ends");
    assert_eq!(toggled(&cx, "huddle-monitor"), Some(false));
    cx.simulate_media_state(mic.clone(), live("Phone microphone"));
    assert!(!mic_piece(&cx).unwrap().3, "and stays off");
}

/// The loudspeaker route, `playing` or not.
fn speaker(playing: bool) -> AudioDevice {
    audio("speaker", "Speaker", playing, AudioKind::Speaker)
}

/// AirPods as a route, `playing` or not.
fn airpods(playing: bool) -> AudioDevice {
    audio("airpods", "AirPods Pro", playing, AudioKind::Headphones)
}

/// Through the loudspeaker the live loop howls: the phone refuses it and
/// says why. Through AirPods it may run, and says what it does.
#[test]
fn hear_myself_is_refused_through_the_speaker() {
    let (mut cx, _ticks) = phone();
    live_pieces(&mut cx);
    assert!(disabled(&cx, "huddle-monitor"), "the speaker howls");
    assert!(cx.has_text(
        "Through the speaker your voice loops back into the microphone and howls. Connect \
         headphones to hear yourself."
    ));
    route(&mut cx, "huddle-sound-1");
    let mic = mic_key(&cx);
    cx.simulate_media_state(mic.clone(), live("AirPods Pro"));
    assert!(!disabled(&cx, "huddle-monitor"));
    assert!(cx.has_text("Headphones connected: hear your own microphone as you speak."));
    cx.simulate_click("huddle-monitor");
    assert!(mic_piece(&cx).unwrap().3, "looping through the AirPods");
}

/// A route picked mid-loop ends the loop, as muting does: the new route
/// could be the loudspeaker.
#[test]
fn picking_a_route_ends_the_live_loop() {
    let (mut cx, _ticks) = phone();
    route(&mut cx, "huddle-sound-1");
    let mic = mic_key(&cx);
    cx.simulate_media_state(mic.clone(), live("AirPods Pro"));
    cx.simulate_click("huddle-monitor");
    assert!(mic_piece(&cx).unwrap().3);
    route(&mut cx, "huddle-sound-2");
    let mic = mic_key(&cx);
    cx.simulate_media_state(mic.clone(), live("Phone microphone"));
    assert!(!mic_piece(&cx).unwrap().3, "the loop ends");
    assert_eq!(toggled(&cx, "huddle-monitor"), Some(false));
}

/// Back closes the open sound sheet, and is there exactly while it is: on
/// the check and in the room it goes nowhere (the host's capsule closes
/// the view; Leave ends the room).
#[test]
fn back_closes_the_sound_sheet_and_else_goes_nowhere() {
    let (mut cx, _ticks) = phone();
    assert_eq!(cx.back_handler(), None);
    cx.simulate_click("huddle-sound");
    assert!(cx.find("huddle-sound-sheet").is_some());
    cx.simulate_back();
    assert!(cx.find("huddle-sound-sheet").is_none());
    assert_eq!(cx.back_handler(), None);
    cx.simulate_click("huddle-join");
    assert_eq!(cx.back_handler(), None);
    cx.simulate_click("huddle-sound");
    cx.simulate_back();
    assert!(cx.find("huddle-sound-sheet").is_none());
    assert!(cx.find("huddle-bar").is_some(), "still in the room");
}

/// Join enters the room where the view is, with every pick kept, and
/// floats nothing, on a host that would float; Leave unfloats nothing.
/// The room counts its time and is honest about who is in it.
#[test]
fn join_and_leave_keep_the_view_where_it_is() {
    let (mut cx, ticks) = phone();
    cx.simulate_click("huddle-flip");
    cx.simulate_click("huddle-join");
    cx.run_until_parked();
    assert!(cx.host().requests::<HostFloat>().is_empty(), "no float");
    assert_eq!(
        cx.host().requests::<HostTitle>().last().map(String::as_str),
        Some("Huddle \u{b7} just you")
    );
    has_all(
        &cx,
        &[
            "00:00 \u{b7} only you",
            "Only you are here. Invite someone, or start a call and share its link.",
            "Leave",
        ],
    );
    let (_, device, mirror, fit) = camera_piece(&cx).unwrap();
    assert_eq!(
        (device.as_deref(), mirror, fit),
        (Some("back"), false, ImageObjectFit::Cover)
    );
    ticks.send(());
    cx.run_until_parked();
    assert!(cx.has_text("00:01 \u{b7} only you"));
    cx.simulate_click("huddle-leave");
    assert!(cx.host().requests::<HostUnfloat>().is_empty(), "no unfloat");
    assert!(cx.has_text("Join huddle"));
    assert_eq!(camera_piece(&cx).unwrap().1.as_deref(), Some("back"));
}

/// Muting lets the microphone go on the check and in the room; in the
/// room speaking rings the tile and marks the chip, and the bar's
/// microphone is slashed on the danger fill while muted.
#[test]
fn the_room_shows_who_speaks_and_mute_lets_the_microphone_go() {
    let (mut cx, _ticks) = phone();
    cx.simulate_click("huddle-mic");
    assert!(mic_piece(&cx).is_none(), "muted on the check");
    assert!(disabled(&cx, "huddle-record"));
    cx.simulate_click("huddle-mic");
    cx.simulate_click("huddle-join");
    let mic = mic_key(&cx);
    cx.simulate_media_state(
        mic.clone(),
        MediaState {
            speaking: true,
            ..live("Phone microphone")
        },
    );
    assert!(cx.find("huddle-mark-speaking").is_some());
    cx.simulate_click("huddle-mic");
    assert!(mic_piece(&cx).is_none(), "muted in the room");
    assert!(cx.find("huddle-mark-muted").is_some());
    assert_eq!(toggled(&cx, "huddle-mic"), Some(true));
    assert_eq!(
        cx.interactivity("huddle-mic").aria.label.as_deref(),
        Some("Unmute")
    );
}

/// Record asks the host for five seconds, counts them down, and Stop ends
/// it; it waits for a live microphone.
#[test]
fn record_then_play_back() {
    let (mut cx, ticks) = phone();
    assert!(disabled(&cx, "huddle-record"), "waits for the microphone");
    live_pieces(&mut cx);
    cx.simulate_click("huddle-record");
    assert_eq!(
        mic_piece(&cx).unwrap().4,
        [MicCommand::Record {
            seconds: RECORD_SECONDS
        }]
    );
    let mic = mic_key(&cx);
    cx.simulate_media_state(
        mic.clone(),
        MediaState {
            playback: Playback::Recording,
            ..live("Phone microphone")
        },
    );
    ticks.send(());
    cx.run_until_parked();
    has_all(&cx, &["Stop", "0:04"]);
    cx.simulate_media_state(
        mic.clone(),
        MediaState {
            playback: Playback::Playing,
            ..live("Phone microphone")
        },
    );
    has_all(&cx, &["Stop playback", "Playing it back\u{2026}"]);
    cx.simulate_click("huddle-record");
    assert_eq!(mic_piece(&cx).unwrap().4, [MicCommand::Stop]);
}

/// A camera slow to open says the phone is asking, and the chip calls no
/// picture mirrored while there is none.
#[test]
fn a_camera_slow_to_open_says_the_phone_is_asking() {
    let (mut cx, ticks) = phone();
    assert!(!cx.has_text("Opening the camera\u{2026}"));
    ticks.send(());
    ticks.send(());
    cx.run_until_parked();
    has_all(
        &cx,
        &[
            "Opening the camera\u{2026}",
            "Allow the camera when your phone asks.",
        ],
    );
    assert!(!cx.has_text("\u{b7} mirrored"));
}

/// A refused camera says so in its card, with Open Settings where the
/// host can open them (and opens exactly the camera's page) and Try
/// again; the microphone and camera buttons stay.
#[test]
fn a_refused_camera_offers_settings_where_the_host_can() {
    let (mut cx, _ticks) = phone();
    let (key, ..) = camera_piece(&cx).unwrap();
    cx.simulate_media_state(key.clone(), status(MediaStatus::Denied));
    has_all(
        &cx,
        &[
            "Camera access is off",
            "Ducktape is not allowed to use the camera. Allow it in Settings, then try again.",
        ],
    );
    assert!(cx.find("huddle-camera-settings").is_none());
    assert!(cx.find("huddle-mic").is_some() && cx.find("huddle-camera").is_some());
    cx.simulate_click("huddle-camera-retry");
    cx.run_until_parked();
    let (key, ..) = camera_piece(&cx).unwrap();
    cx.simulate_media_state(
        key.clone(),
        MediaState {
            status: MediaStatus::Denied,
            opens_settings: true,
            ..Default::default()
        },
    );
    cx.simulate_click("huddle-camera-settings");
    assert_eq!(cx.host().requests::<HostShow>(), [Place::CameraSettings]);
}

/// A camera taken over says what the host wrote, else the phone's own
/// line; a phone with no camera offers joining with the microphone alone.
#[test]
fn a_camera_taken_or_missing_says_so() {
    let (mut cx, _ticks) = phone();
    let (key, ..) = camera_piece(&cx).unwrap();
    cx.simulate_media_state(key.clone(), status(MediaStatus::Gone));
    has_all(
        &cx,
        &[
            "The camera stopped",
            "A phone call or another app took it over. When that is done, try again.",
        ],
    );
    cx.simulate_click("huddle-camera-retry");
    cx.run_until_parked();
    let (key, ..) = camera_piece(&cx).unwrap();
    cx.simulate_media_state(
        key.clone(),
        MediaState {
            status: MediaStatus::Gone,
            detail: "A FaceTime call took the camera.".into(),
            ..Default::default()
        },
    );
    assert!(cx.has_text("A FaceTime call took the camera."));
    cx.simulate_click("huddle-camera-retry");
    cx.run_until_parked();
    let (key, ..) = camera_piece(&cx).unwrap();
    cx.simulate_media_state(key.clone(), status(MediaStatus::NoDevice));
    has_all(
        &cx,
        &[
            "No camera on this device",
            "Join with your microphone only.",
        ],
    );
    cx.simulate_click("huddle-camera-without");
    assert!(cx.find("huddle-bar").is_some(), "in the room");
    assert!(camera_piece(&cx).is_none(), "joined without the camera");
}

/// In the room a phone with no camera says so, with Try again, and no
/// word of joining: the person has joined already.
#[test]
fn a_missing_camera_in_the_room_offers_no_join() {
    let (mut cx, _ticks) = phone();
    cx.simulate_click("huddle-join");
    let (key, ..) = camera_piece(&cx).unwrap();
    cx.simulate_media_state(key.clone(), status(MediaStatus::NoDevice));
    assert!(cx.has_text("No camera on this device"));
    assert!(cx.find("huddle-camera-retry").is_some());
    assert!(cx.find("huddle-camera-without").is_none());
    assert!(!cx.has_text("Join with your microphone only."));
}

/// A microphone in trouble on the check is the Sound row's one line, told
/// to assistive technology on the route too, with Try again under it
/// opening a new piece; the speaker test still sounds, Record and the
/// live loop wait.
#[test]
fn a_microphone_in_trouble_is_one_line_with_try_again() {
    let (mut cx, _ticks) = phone();
    let key = mic_key(&cx);
    cx.simulate_media_state(key.clone(), status(MediaStatus::Denied));
    assert!(cx.has_text("Microphone access is off. Allow it in Settings, then try again."));
    assert_eq!(
        cx.interactivity("huddle-sound").aria.description.as_deref(),
        Some("Microphone access is off")
    );
    assert!(cx.find("huddle-meter").is_none());
    assert!(!disabled(&cx, "huddle-sound-test"), "the speaker sounds");
    for control in ["huddle-record", "huddle-monitor"] {
        assert!(disabled(&cx, control), "{control} waits");
    }
    cx.simulate_click("huddle-microphone-retry");
    cx.run_until_parked();
    let retried = mic_key(&cx);
    assert_ne!(retried, key, "a new piece");
    cx.simulate_media_state(
        retried.clone(),
        MediaState {
            status: MediaStatus::Gone,
            detail: "A phone call took the microphone.".into(),
            ..Default::default()
        },
    );
    assert!(cx.has_text("The microphone stopped. A phone call took the microphone."));
}

/// A refused microphone offers Open Settings where the host says its
/// settings can help, and opens exactly the microphone's page; Try again
/// stays beside it (a grant there does not reopen the piece). On the
/// check under the Sound row, in the room over the stage.
#[test]
fn a_refused_microphone_offers_settings_where_the_host_can() {
    let (mut cx, _ticks) = phone();
    let key = mic_key(&cx);
    cx.simulate_media_state(key.clone(), status(MediaStatus::Denied));
    assert!(cx.find("huddle-microphone-settings").is_none());
    cx.simulate_click("huddle-microphone-retry");
    cx.run_until_parked();
    let key = mic_key(&cx);
    cx.simulate_media_state(
        key.clone(),
        MediaState {
            status: MediaStatus::Denied,
            opens_settings: true,
            ..Default::default()
        },
    );
    assert!(cx.find("huddle-microphone-retry").is_some());
    cx.simulate_click("huddle-microphone-settings");
    assert_eq!(
        cx.host().requests::<HostShow>(),
        [Place::MicrophoneSettings]
    );

    // still refused in the room: said over the stage, with the same fixes
    cx.simulate_click("huddle-join");
    assert!(cx.find("huddle-microphone-retry").is_some());
    cx.simulate_click("huddle-microphone-settings");
    assert_eq!(
        cx.host().requests::<HostShow>(),
        [Place::MicrophoneSettings, Place::MicrophoneSettings]
    );
}

/// A microphone in trouble in the room is said over the stage as an
/// alert, at once, with Try again in the check's word, and its bar button
/// is slashed on the danger fill.
#[test]
fn a_microphone_in_trouble_in_the_room_says_so() {
    let (mut cx, _ticks) = phone();
    cx.simulate_click("huddle-join");
    let key = mic_key(&cx);
    cx.simulate_media_state(key.clone(), status(MediaStatus::Busy));
    has_all(&cx, &["Another app is using the microphone.", "Try again"]);
    let band = cx.interactivity("huddle-mic-trouble");
    assert_eq!(band.role, Some(Role::Alert), "said at once");
    assert_eq!(band.aria.live, Some(view::wire::Live::Assertive));
    cx.simulate_click("huddle-microphone-retry");
    cx.run_until_parked();
    assert_ne!(mic_key(&cx), key, "a new piece");
}
