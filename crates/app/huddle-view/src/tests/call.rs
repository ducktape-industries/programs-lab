//! Calls on a fake host whose program is the real one: the `huddle`
//! program over a mock store answers the view's queries and runs its ops
//! as the reader's account, and the test speaks for the node's live side
//! (the room's track list, what each piece tells). The desktop's tree under
//! a pointer; the phone's incoming screen under a finger.
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use ::program::{Env, MockHost, Program};
use huddle::{Huddle as Calls, Op};
use view::methods::{
    Change, Changes, HostId, HostRoute, HostSession, HostUnfloat, LiveAnnounce, LiveMode,
    LiveTrack, LiveTracks, Query as Ask, Session, Submit, Watching,
};
use view::testing::{Relay, StreamSender, TestAppContext};
use view::wire::{MediaState, MediaStatus, Node, RemoteMedia, TrackState};

use super::{hosted, walk};
use crate::Huddle;

/// The reader, and the people who call them.
const ME: u64 = 1;
const ADA: u64 = 2;
const BO: u64 = 3;

/// The call the host mints, and the one Ada starts.
const MINTED: &str = "call-18f2c0a1-1";
const ADAS: &str = "call-ada";

/// The chain as the view reads it, the ops it sent, and the feeds the test
/// speaks for the host through.
struct Line {
    cx: TestAppContext,
    view: view::Entity<Huddle>,
    chain: MockHost,
    /// The chain's time, in seconds.
    now: Rc<Cell<u64>>,
    sent: Rc<RefCell<Vec<Op>>>,
    changes: StreamSender<Changes<Calls>>,
    route: StreamSender<HostRoute>,
    ticks: StreamSender<view::methods::ClockTicks>,
}

fn env(now: u64) -> Env {
    Env {
        time: now * 1000,
        ..MockHost::env(huddle::PROGRAM)
    }
}

/// `op` as `who` on `chain` at `now`; a refusal is the program's.
fn run(chain: &MockHost, now: u64, who: u64, op: Op) -> Result<(), view::host::Error> {
    let ctx = chain.exec(env(now).signed(who.to_be_bytes(), Some(who)));
    chain
        .attempt(|| Calls::execute(&ctx, op))
        .map_err(|refusal| view::host::Error::new(refusal.code, refusal.message))
}

/// The view open in the desk's pane, the reader seated, over a chain with
/// the huddle program and people named Ada and Bo.
fn line(touch: bool) -> Line {
    seated(ME, MockHost::default(), Rc::new(Cell::new(1_000)), touch)
}

/// The view open as account `me`, over `chain` at `now`.
fn seated(me: u64, chain: MockHost, now: Rc<Cell<u64>>, touch: bool) -> Line {
    let (mut cx, ticks) = hosted();
    let sent = Rc::new(RefCell::new(Vec::new()));
    let (asked, at) = (chain.clone(), now.clone());
    cx.host().handle::<Ask<Calls>>(move |query| {
        Calls::query(&asked.query(env(at.get())), query)
            .map_err(|refusal| view::host::Error::new(refusal.code, refusal.message))
    });
    let (ran, at, log) = (chain.clone(), now.clone(), sent.clone());
    cx.host().handle::<Submit<Calls>>(move |op| {
        log.borrow_mut().push(op.clone());
        Ok(match run(&ran, at.get(), me, op) {
            Ok(()) => view::testing::applied(Vec::new()),
            Err(refusal) => view::methods::Submitted::Refused {
                frame: [0; 32],
                until: 0,
                refusal,
            },
        })
    });
    cx.host().handle::<Ask<identity::Identity>>(|query| {
        let person = |number, name: &str| identity::Profile {
            number,
            name: name.into(),
            kind: identity::Kind::User,
        };
        Ok(match query {
            identity::Query::Profile { number: ADA } => {
                identity::Reply::Profile(Some(person(ADA, "ada")))
            }
            identity::Query::Profile { number: BO } => {
                identity::Reply::Profile(Some(person(BO, "bo")))
            }
            identity::Query::Profile { .. } => identity::Reply::Profile(None),
            identity::Query::Profiles { .. } => identity::Reply::Profiles {
                profiles: vec![person(ME, "me"), person(ADA, "ada"), person(BO, "bo")],
                next: None,
            },
            query => panic!("unexpected identity query: {query:?}"),
        })
    });
    cx.host().handle::<HostId>(|_| Ok(MINTED.into()));
    let sessions = cx.host().stream::<HostSession>();
    let changes = cx.host().stream::<Changes<Calls>>();
    let route = cx.host().stream::<HostRoute>();
    let view = cx.open::<Huddle>();
    if touch {
        cx.simulate_touch(true);
        cx.simulate_resize(390., 844.);
    }
    sessions.send(Session {
        connected: true,
        chain_id: "lab-0a1b2c3d".into(),
        signer: "0102".into(),
        account: Some(me),
        endpoint: String::new(),
    });
    cx.run_until_parked();
    Line {
        cx,
        view,
        chain,
        now,
        sent,
        changes,
        route,
        ticks,
    }
}

impl Line {
    /// `op` as `who`, and the block that wrote it reaching the view.
    fn block(&mut self, who: u64, op: Op) {
        if let Err(refusal) = run(&self.chain, self.now.get(), who, op) {
            panic!("refused: {refusal:?}");
        }
        self.wrote();
    }

    /// A block that wrote the calls reaching the view.
    fn wrote(&mut self) {
        let keys = vec![huddle::state::CALLS.key(&String::new())];
        self.changes.send(Some(Change { height: 2, keys }));
        self.cx.run_until_parked();
    }

    /// The room's track list, as the node tells the view.
    fn tracks(&self) -> StreamSender<LiveTracks> {
        self.cx.host().stream::<LiveTracks>()
    }

    /// `who`'s track `name` up or down in the room.
    fn announce(&mut self, tracks: &StreamSender<LiveTracks>, name: String, who: u64, up: bool) {
        let mode = match name.starts_with("mic/") {
            true => LiveMode::Datagram,
            false => LiveMode::Stream,
        };
        tracks.send(Watching::Announce(LiveAnnounce {
            name,
            by: who,
            mode,
            up,
        }));
        self.cx.run_until_parked();
    }

    /// `who` in the call: their microphone's track up.
    fn arrives(&mut self, tracks: &StreamSender<LiveTracks>, who: u64) {
        self.announce(tracks, huddle::microphone_track(who), who, true);
    }

    fn joins(&self) -> usize {
        let sent = self.sent.borrow();
        sent.iter()
            .filter(|op| matches!(op, Op::Join { .. }))
            .count()
    }

    /// The pieces in the tree: each publishing piece's track and each
    /// remote piece's, by kind.
    fn pieces(&self) -> Pieces {
        let mut pieces = Pieces::default();
        walk(self.cx.root(), &mut |node| match node {
            Node::Camera { publish, .. } => pieces.camera = Some(publish.clone()),
            Node::Microphone { publish, muted, .. } => {
                pieces.microphone = Some((publish.clone(), *muted))
            }
            Node::Remote { track, media, .. } => match media {
                RemoteMedia::Audio { .. } => pieces.voices.push(track.name.clone()),
                RemoteMedia::Video { .. } => pieces.pictures.push(track.name.clone()),
            },
            _ => {}
        });
        pieces
    }

    /// The tiles on the stage, in order, by their ids.
    fn tiles(&self) -> Vec<String> {
        let mut tiles = Vec::new();
        walk(self.cx.root(), &mut |node| {
            if let Some(name) = node.identity().and_then(|id| id.name())
                && name.starts_with("huddle-tile-")
            {
                tiles.push(name.to_owned());
            }
        });
        tiles
    }

    /// `media` from the piece `name`.
    fn tell(&mut self, name: &str, media: MediaState) {
        self.cx.simulate_media_state(name.to_owned(), media);
        self.cx.run_until_parked();
    }

    /// A second on the view's clock.
    fn second(&mut self) {
        self.ticks.send(());
        self.cx.run_until_parked();
    }
}

#[derive(Default, Debug)]
struct Pieces {
    camera: Option<Option<LiveTrack>>,
    microphone: Option<(Option<LiveTrack>, bool)>,
    voices: Vec<String>,
    pictures: Vec<String>,
}

fn track(room: &str, name: String) -> Option<LiveTrack> {
    Some(LiveTrack {
        room: room.into(),
        name,
    })
}

fn waiting() -> MediaState {
    MediaState {
        status: MediaStatus::Live,
        track: TrackState::Waiting,
        ..MediaState::default()
    }
}

fn up() -> MediaState {
    MediaState {
        status: MediaStatus::Live,
        track: TrackState::Up { subscribers: 1 },
        ..MediaState::default()
    }
}

fn speaking(speaking: bool) -> MediaState {
    MediaState {
        status: MediaStatus::Live,
        speaking,
        ..up()
    }
}

/// The key of your microphone's piece.
fn mic(line: &Line) -> String {
    super::mic_key(&line.cx)
}

/// Ada calls: the ring is a card on the check, named for her; Answer joins
/// at once, the microphone on, both your pieces publishing into your own
/// tracks of her call, and the card gone.
#[test]
fn a_ring_is_a_card_on_the_check_and_answer_joins_at_once() {
    let mut line = line(false);
    assert!(line.cx.find(&format!("huddle-ring-{ADAS}")).is_none());
    // muted on the check: answering turns the microphone on
    line.cx.simulate_click("huddle-mic");
    line.block(
        ADA,
        Op::Start {
            call: ADAS.into(),
            invited: vec![ME],
        },
    );
    assert!(line.cx.has_text("ada is calling"), "{:?}", line.cx.texts());
    line.cx.simulate_click(&format!("huddle-answer-{ADAS}"));
    line.cx.run_until_parked();
    assert_eq!(line.joins(), 1);
    assert!(line.cx.find("huddle-bar").is_some(), "in the room");
    let pieces = line.pieces();
    assert_eq!(pieces.camera, Some(track(ADAS, "cam/1".into())));
    assert_eq!(
        pieces.microphone,
        Some((track(ADAS, "mic/1".into()), false))
    );
    assert!(!line.cx.has_text("ada is calling"));
}

/// Decline takes the ring away at once and tells the program.
#[test]
fn decline_takes_the_ring_away() {
    let mut line = line(false);
    line.block(
        ADA,
        Op::Start {
            call: ADAS.into(),
            invited: vec![ME],
        },
    );
    line.cx.simulate_click(&format!("huddle-decline-{ADAS}"));
    line.cx.run_until_parked();
    assert!(!line.cx.has_text("ada is calling"));
    assert_eq!(
        line.sent.borrow().last(),
        Some(&Op::Decline { call: ADAS.into() })
    );
    assert!(line.cx.find("huddle-join").is_some(), "still on the check");
}

/// A link to a call (`call/<id>`, a ring's) joins it at once, from the
/// check.
#[test]
fn a_call_link_joins_at_once() {
    let mut line = line(false);
    line.block(
        ADA,
        Op::Start {
            call: ADAS.into(),
            invited: vec![ME],
        },
    );
    line.route.send(format!("call/{ADAS}"));
    line.cx.run_until_parked();
    assert_eq!(line.joins(), 1);
    assert!(line.cx.find("huddle-bar").is_some(), "in the room");
    assert_eq!(
        line.pieces().microphone,
        Some((track(ADAS, "mic/1".into()), false))
    );
}

/// A ring's end link (`call/<id>/leave`, opened when the answered call is
/// ended from the system's call screen) leaves that call as Leave does,
/// joining nothing first; an end for another call, or with no call, does
/// nothing. An end right behind its answer finds the call answered.
#[test]
fn an_end_link_leaves_its_call_and_no_other() {
    let mut line = line(false);
    line.block(
        ADA,
        Op::Start {
            call: ADAS.into(),
            invited: vec![ME],
        },
    );
    line.route.send(format!("call/{ADAS}"));
    line.cx.run_until_parked();
    assert_eq!(line.joins(), 1);

    // another call's end: still in Ada's
    line.route.send("call/call-old/leave".into());
    line.cx.run_until_parked();
    assert!(line.cx.find("huddle-bar").is_some(), "still in the room");
    assert!(
        !line
            .sent
            .borrow()
            .iter()
            .any(|op| matches!(op, Op::Leave { .. }))
    );

    line.route.send(format!("call/{ADAS}/leave"));
    line.cx.run_until_parked();
    assert_eq!(
        line.sent.borrow().last(),
        Some(&Op::Leave { call: ADAS.into() })
    );
    assert_eq!(line.joins(), 1, "no Join on the way out");
    assert!(line.cx.find("huddle-join").is_some(), "on the check");
    let pieces = line.pieces();
    assert_eq!(pieces.camera, Some(None), "publishing nothing");
    assert_eq!(pieces.microphone, Some((None, false)), "no hot mic");

    // out of every call: the same end again sends nothing
    let sent = line.sent.borrow().len();
    line.route.send(format!("call/{ADAS}/leave"));
    line.cx.run_until_parked();
    assert_eq!(line.sent.borrow().len(), sent);

    // answered and ended in one go: the end finds the answer still reading
    // the call, and calls it off; nothing is joined to be left
    line.route.send(format!("call/{ADAS}"));
    line.route.send(format!("call/{ADAS}/leave"));
    line.cx.run_until_parked();
    assert_eq!(line.sent.borrow().len(), sent, "{:?}", line.sent.borrow());
    assert!(line.cx.find("huddle-join").is_some(), "on the check");
    assert_eq!(line.pieces().microphone, Some((None, false)), "no hot mic");
}

/// Invite in a room with no call starts one, ringing whoever was picked,
/// under the id the host mints; the pieces then publish into it. The
/// tracks' `Waiting` sends one Join, however many say it; one more once a
/// track was up and waits again.
#[test]
fn invite_starts_a_call_and_a_waiting_track_sends_one_join() {
    let mut line = line(false);
    line.cx.simulate_click("huddle-join");
    line.cx.run_until_parked();
    assert_eq!(line.pieces().microphone, Some((None, false)), "no call yet");
    line.cx.simulate_click("huddle-invite");
    line.cx.run_until_parked();
    assert!(line.cx.has_text("bo"), "{:?}", line.cx.texts());
    assert!(!line.cx.has_text("me"), "you are not listed");
    // ada, then bo: you are not on the list
    line.cx.simulate_click("huddle-menu-1");
    line.cx.run_until_parked();
    assert_eq!(
        line.sent.borrow().as_slice(),
        [Op::Start {
            call: MINTED.into(),
            invited: vec![BO],
        }]
    );
    assert_eq!(
        line.pieces().microphone,
        Some((track(MINTED, "mic/1".into()), false))
    );
    let tracks = line.tracks();
    tracks.send(Watching::Waiting);
    line.cx.run_until_parked();
    let key = mic(&line);
    line.tell(&key, waiting());
    assert_eq!(line.joins(), 1, "one Join for the Waiting");
    line.tell(&key, up());
    line.tell(&key, waiting());
    line.tell(&key, waiting());
    assert_eq!(line.joins(), 2, "one more for the next");
    // asked in, Bo is rung: no one else is in the room yet
    assert!(line.cx.has_text("00:00 \u{b7} only you"));
}

/// In a call, muting keeps the microphone's piece and its track (your
/// presence), muted; leaving says Leave and goes back to the check.
#[test]
fn muted_in_a_call_you_stay_in_it_and_leave_leaves_it() {
    let mut line = line(false);
    line.block(
        ADA,
        Op::Start {
            call: ADAS.into(),
            invited: vec![ME],
        },
    );
    line.cx.simulate_click(&format!("huddle-answer-{ADAS}"));
    line.cx.run_until_parked();
    line.cx.simulate_click("huddle-mic");
    assert_eq!(
        line.pieces().microphone,
        Some((track(ADAS, "mic/1".into()), true))
    );
    line.cx.simulate_click("huddle-leave");
    line.cx.run_until_parked();
    assert_eq!(
        line.sent.borrow().last(),
        Some(&Op::Leave { call: ADAS.into() })
    );
    assert!(line.cx.find("huddle-join").is_some(), "on the check");
    assert_eq!(line.pieces().camera, Some(None), "publishing nothing");
}

/// A second microphone track up turns "only you" into "· 2": a voice for
/// them, a tile with their face, and with their camera up their picture.
#[test]
fn a_second_voice_turns_only_you_into_two() {
    let mut line = line(false);
    line.block(
        ADA,
        Op::Start {
            call: ADAS.into(),
            invited: vec![ME],
        },
    );
    line.cx.simulate_click(&format!("huddle-answer-{ADAS}"));
    line.cx.run_until_parked();
    let tracks = line.tracks();
    // your own track, announced to you too, is not another person
    line.arrives(&tracks, ME);
    assert!(line.cx.has_text("00:00 \u{b7} only you"));
    line.arrives(&tracks, ADA);
    assert!(line.cx.has_text("00:00 \u{b7} 2"), "{:?}", line.cx.texts());
    assert!(line.cx.find("huddle-solo").is_none(), "not alone");
    assert_eq!(line.pieces().voices, ["mic/2"]);
    assert!(line.pieces().pictures.is_empty(), "no camera up: her face");
    assert!(line.cx.has_text("ada"));
    line.announce(&tracks, "cam/2".into(), ADA, true);
    assert_eq!(line.pieces().pictures, ["cam/2"]);
    assert_eq!(
        line.cx.interactivity("huddle-count").aria.label.as_deref(),
        Some("2 in the huddle")
    );
    // she goes: only you again
    line.announce(&tracks, "mic/2".into(), ADA, false);
    assert!(line.cx.has_text("00:00 \u{b7} only you"));
    assert!(line.pieces().voices.is_empty());
}

/// The last one to start speaking takes the main tile (the stage's
/// first), each holder keeping it two seconds; never you with others there.
#[test]
fn the_speaker_takes_the_main_tile() {
    let mut line = line(false);
    line.block(
        ADA,
        Op::Start {
            call: ADAS.into(),
            invited: vec![ME, BO],
        },
    );
    line.cx.simulate_click(&format!("huddle-answer-{ADAS}"));
    line.cx.run_until_parked();
    let tracks = line.tracks();
    line.arrives(&tracks, ADA);
    line.arrives(&tracks, BO);
    assert_eq!(
        line.tiles(),
        ["huddle-tile-2", "huddle-tile-3", "huddle-tile-you"],
        "before anyone speaks, the first other"
    );
    // you speak: you stay last
    let key = mic(&line);
    line.tell(&key, speaking(true));
    assert_eq!(line.tiles()[0], "huddle-tile-2");
    line.tell("huddle-voice-3", speaking(true));
    assert_eq!(line.tiles()[0], "huddle-tile-3");
    // Ada starts while Bo still speaks: Bo holds the tile two seconds,
    // then the last to start has it, the lower number or not
    line.tell("huddle-voice-2", speaking(true));
    assert_eq!(line.tiles()[0], "huddle-tile-3", "held");
    line.second();
    assert_eq!(line.tiles()[0], "huddle-tile-3", "held");
    line.second();
    assert_eq!(line.tiles()[0], "huddle-tile-2", "handed on");
    // Bo starts again: Ada had it two seconds already
    line.tell("huddle-voice-3", speaking(false));
    line.second();
    line.second();
    line.tell("huddle-voice-3", speaking(true));
    assert_eq!(line.tiles()[0], "huddle-tile-3", "the last to start");
    // quiet, the holder keeps it until someone starts
    line.tell("huddle-voice-3", speaking(false));
    line.tell("huddle-voice-2", speaking(false));
    for _ in 0..3 {
        line.second();
    }
    assert_eq!(line.tiles()[0], "huddle-tile-3", "kept");
}

/// Seven in the call: six on the stage, you among them, and "+1".
#[test]
fn seven_in_a_call_are_six_tiles_and_one_more() {
    let mut line = line(false);
    line.block(
        ADA,
        Op::Start {
            call: ADAS.into(),
            invited: vec![ME],
        },
    );
    line.cx.simulate_click(&format!("huddle-answer-{ADAS}"));
    line.cx.run_until_parked();
    let tracks = line.tracks();
    for who in 2..8 {
        line.arrives(&tracks, who);
    }
    assert!(line.cx.has_text("00:00 \u{b7} 7"));
    assert_eq!(line.tiles().len(), 6, "{:?}", line.tiles());
    assert!(line.tiles().contains(&"huddle-tile-you".to_owned()));
    assert!(line.cx.has_text("+1"));
    // everyone is heard, on the stage or not
    assert_eq!(line.pieces().voices.len(), 6);
}

/// The node ends the room's track list `not_granted` (the caller ended
/// the call, and your grant went): back to the check, with why.
#[test]
fn a_call_ended_under_you_says_so() {
    let mut line = line(false);
    line.block(
        ADA,
        Op::Start {
            call: ADAS.into(),
            invited: vec![ME],
        },
    );
    line.cx
        .host()
        .refuse::<LiveTracks>("not_granted", "the grant was taken back");
    line.cx.simulate_click(&format!("huddle-answer-{ADAS}"));
    line.cx.run_until_parked();
    assert!(line.cx.find("huddle-join").is_some(), "on the check");
    assert!(line.cx.has_text("The call ended."));
}

/// A track that does not come through says so in the network row.
#[test]
fn a_slow_track_says_reconnecting() {
    let mut line = line(false);
    line.block(
        ADA,
        Op::Start {
            call: ADAS.into(),
            invited: vec![ME],
        },
    );
    line.cx.simulate_click(&format!("huddle-answer-{ADAS}"));
    line.cx.run_until_parked();
    let key = mic(&line);
    line.tell(&key, up());
    let tracks = line.tracks();
    line.arrives(&tracks, ADA);
    assert!(
        line.cx.find("huddle-network").is_none(),
        "{:?}",
        line.cx.texts()
    );
    line.tell(
        "huddle-voice-2",
        MediaState {
            status: MediaStatus::Starting,
            ..MediaState::default()
        },
    );
    line.second();
    assert!(line.cx.find("huddle-network").is_none(), "a second is fine");
    line.second();
    line.second();
    assert!(line.cx.has_text("Reconnecting\u{2026}"));
    line.tell("huddle-voice-2", speaking(false));
    assert!(line.cx.find("huddle-network").is_none());
}

/// On a phone a ring takes the whole screen; Answer joins at once, in
/// place.
#[test]
fn on_a_phone_a_ring_is_the_whole_screen() {
    let mut line = line(true);
    line.block(
        ADA,
        Op::Start {
            call: ADAS.into(),
            invited: vec![ME, BO],
        },
    );
    assert!(line.cx.has_text("ada is calling"), "{:?}", line.cx.texts());
    assert!(line.cx.has_text("Huddle with 3 people"));
    assert!(
        line.cx.find("huddle-join").is_none(),
        "the check is under it"
    );
    line.cx.simulate_click("huddle-answer");
    line.cx.run_until_parked();
    assert_eq!(line.joins(), 1);
    assert!(line.cx.find("huddle-bar").is_some(), "in the room");
    assert_eq!(
        line.pieces().microphone,
        Some((track(ADAS, "mic/1".into()), false))
    );
    // Ada is there: she fills the stage, you a thumbnail
    let tracks = line.tracks();
    line.arrives(&tracks, ADA);
    assert_eq!(line.tiles(), ["huddle-tile-2", "huddle-tile-you"]);
}

/// Each view runs until neither has anything left from the other: what
/// one frame asks of the relay lands in the other's host.
fn settle(lines: &mut [&mut Line]) {
    for _ in 0..4 {
        for line in lines.iter_mut() {
            line.cx.run_until_parked();
        }
    }
}

/// Two views in one call over the test relay, each its own account on one
/// chain: Ada rings me from her room's Invite and I answer; each plays the
/// other's voice on their tile. Ada leaving ends her track under me and
/// takes her tile away; I stay in the call alone.
#[test]
fn two_views_call_each_other_over_the_relay() {
    let (chain, now) = (MockHost::default(), Rc::new(Cell::new(1_000)));
    let mut me = seated(ME, chain.clone(), now.clone(), false);
    let mut ada = seated(ADA, chain, now, false);
    let relay = Relay::new();
    relay.link(&me.cx).link(&ada.cx);
    // the rows the call's Start writes: each one asked in, never `*`
    relay.hold(MINTED);
    for who in [ME, ADA] {
        relay.grant(MINTED, Some(who), huddle::seat());
    }

    ada.cx.simulate_click("huddle-join");
    ada.cx.simulate_click("huddle-invite");
    ada.cx.run_until_parked();
    // me, then bo: Ada is not on her own list
    ada.cx.simulate_click("huddle-menu-0");
    ada.wrote();
    me.wrote();
    let answer = format!("huddle-answer-{MINTED}");
    assert!(me.cx.has_text("ada is calling"), "{:?}", me.cx.texts());
    me.cx.simulate_click(&answer);
    settle(&mut [&mut me, &mut ada]);

    assert_eq!(me.pieces().voices, ["mic/2"]);
    assert_eq!(ada.pieces().voices, ["mic/1"]);
    assert!(me.tiles().contains(&"huddle-tile-2".to_owned()));
    assert!(ada.tiles().contains(&"huddle-tile-1".to_owned()));
    assert_eq!((me.joins(), ada.joins()), (1, 0), "the rows were up");

    ada.cx.simulate_click("huddle-leave");
    relay.revoke(MINTED, Some(ADA));
    settle(&mut [&mut me, &mut ada]);
    assert_eq!(
        ada.sent.borrow().last(),
        Some(&Op::Leave {
            call: MINTED.into()
        })
    );
    assert!(me.pieces().voices.is_empty(), "her voice ended");
    assert!(!me.tiles().contains(&"huddle-tile-2".to_owned()));
    assert!(me.cx.find("huddle-leave").is_some(), "still in the call");
}

/// A ring runs out on the chain's clock: an invitee who never answered a
/// call that goes on without them sees its card go once the ring is asked
/// again.
#[test]
fn a_ring_that_ran_out_is_no_card() {
    let mut line = line(false);
    line.block(
        ADA,
        Op::Start {
            call: ADAS.into(),
            invited: vec![ME, BO],
        },
    );
    line.block(BO, Op::Join { call: ADAS.into() });
    let card = format!("huddle-ring-{ADAS}");
    assert!(line.cx.find(&card).is_some(), "ringing");
    line.now.set(1_000 + 105);
    for _ in 0..5 {
        line.second();
    }
    assert!(line.cx.find(&card).is_none(), "{:?}", line.cx.texts());
}

/// On a phone, a call that cannot go on says why in the room: here a link
/// to a call that is over (its ring and its wait for knocks ran out), whose
/// Join is refused.
#[test]
fn on_a_phone_the_room_says_why_a_call_did_not_go_on() {
    let mut line = line(true);
    line.block(
        ADA,
        Op::Start {
            call: ADAS.into(),
            invited: vec![ME],
        },
    );
    line.now.set(1_000 + huddle::WAIT_SECONDS);
    line.route.send(format!("call/{ADAS}"));
    line.cx.run_until_parked();
    assert!(line.cx.find("huddle-leave").is_some(), "in the room");
    assert_eq!(
        line.sent.borrow().as_slice(),
        [Op::Join { call: ADAS.into() }]
    );
    assert!(
        line.cx
            .has_text("The call did not go through: call call-ada is over"),
        "{:?}",
        line.cx.texts()
    );
}

/// A link to a call its caller ended, or to no call, says so on the check
/// and joins nothing.
#[test]
fn a_link_to_an_ended_or_unknown_call_says_so_on_the_check() {
    let mut line = line(false);
    line.block(
        ADA,
        Op::Start {
            call: ADAS.into(),
            invited: vec![ME],
        },
    );
    line.block(ADA, Op::End { call: ADAS.into() });
    line.route.send(format!("call/{ADAS}"));
    line.cx.run_until_parked();
    assert!(line.cx.find("huddle-join").is_some(), "on the check");
    assert!(line.cx.has_text("The call ended."), "{:?}", line.cx.texts());
    line.route.send("call/nowhere".into());
    line.cx.run_until_parked();
    assert!(line.cx.has_text("There is no such call."));
    assert!(line.sent.borrow().is_empty(), "{:?}", line.sent.borrow());
}

/// The node ends the room's track list under you: out of the room, the
/// pane back on the desk. A grant taken back leaves nothing to leave; any
/// other end (no live connection) says Leave, so no one waits on you.
#[test]
fn a_call_ended_under_you_unfloats_and_says_leave() {
    for (code, says_leave) in [("not_granted", false), ("live_unavailable", true)] {
        let mut line = line(false);
        line.block(
            ADA,
            Op::Start {
                call: ADAS.into(),
                invited: vec![ME],
            },
        );
        line.cx
            .host()
            .refuse::<LiveTracks>(code, "the room is gone");
        line.cx.simulate_click(&format!("huddle-answer-{ADAS}"));
        line.cx.run_until_parked();
        assert!(
            line.cx.find("huddle-join").is_some(),
            "{code}: on the check"
        );
        assert_eq!(
            line.cx.host().requests::<HostUnfloat>().len(),
            1,
            "{code}: back on the desk"
        );
        let left = line
            .sent
            .borrow()
            .contains(&Op::Leave { call: ADAS.into() });
        assert_eq!(left, says_leave, "{code}");
    }
}

/// You leave the room before the Start of your call lands: the call is
/// left at once, so no one is rung into an empty room.
#[test]
fn a_start_landing_after_you_left_says_leave() {
    let mut line = line(false);
    line.cx.simulate_click("huddle-join");
    let view = line.view.clone();
    line.cx.update(&view, |view, _, cx| view.invite(ADA, cx));
    // the minted id lands, and Start goes out; its answer is still owed
    for _ in 0..4 {
        if !line.sent.borrow().is_empty() {
            break;
        }
        line.cx.tick(Vec::new());
    }
    line.cx.update(&view, |view, _, cx| view.leave(cx));
    line.cx.run_until_parked();
    let start = Op::Start {
        call: MINTED.into(),
        invited: vec![ADA],
    };
    let leave = Op::Leave {
        call: MINTED.into(),
    };
    assert_eq!(line.sent.borrow().as_slice(), [start, leave]);
    let call = Calls::query(
        &line.chain.query(env(line.now.get())),
        huddle::Query::Call { id: MINTED.into() },
    );
    let Ok(huddle::Reply::Call(Some(call))) = call else {
        panic!("the call is kept");
    };
    assert!(call.ended.is_some(), "no one is rung into it");
}

/// Why an op of the call did not go through is the room's line for five
/// seconds, then the call goes on without it; while a track of yours is
/// not up, "Reconnecting…" is the line instead.
#[test]
fn a_note_in_the_room_gives_way_to_reconnecting_and_goes() {
    let mut line = line(false);
    line.block(
        ADA,
        Op::Start {
            call: ADAS.into(),
            invited: vec![ME],
        },
    );
    line.now.set(1_000 + huddle::WAIT_SECONDS);
    line.route.send(format!("call/{ADAS}"));
    line.cx.run_until_parked();
    let note = "The call did not go through: call call-ada is over";
    assert!(line.cx.has_text(note), "{:?}", line.cx.texts());
    let key = mic(&line);
    line.tell(&key, waiting());
    assert!(line.cx.has_text("Reconnecting\u{2026}"));
    assert!(!line.cx.has_text(note), "reconnecting wins");
    line.tell(&key, up());
    assert!(line.cx.has_text(note), "said again, from now");
    for _ in 0..5 {
        line.second();
    }
    assert!(line.cx.find("huddle-network").is_none(), "gone");
    assert!(line.cx.find("huddle-leave").is_some(), "still in the room");
}

/// The room lists tracks in no set order: Ada's camera up before her
/// microphone (her row first lets her publish the camera alone) still
/// shows her picture once her microphone brings her in.
#[test]
fn a_camera_heard_before_its_microphone_still_shows() {
    let (chain, now) = (MockHost::default(), Rc::new(Cell::new(1_000)));
    let mut me = seated(ME, chain.clone(), now.clone(), false);
    let mut ada = seated(ADA, chain, now, false);
    let relay = Relay::new();
    relay.link(&me.cx).link(&ada.cx);
    relay.hold(MINTED);
    relay.grant(MINTED, Some(ME), huddle::seat());
    let camera_only = ::program::live::Grant {
        publish: vec!["cam/{account}".into()],
        subscribe: vec!["*".into()],
    };
    relay.grant(MINTED, Some(ADA), camera_only);

    ada.cx.simulate_click("huddle-join");
    ada.cx.simulate_click("huddle-invite");
    ada.cx.run_until_parked();
    ada.cx.simulate_click("huddle-menu-0");
    ada.wrote();
    me.wrote();
    me.cx.simulate_click(&format!("huddle-answer-{MINTED}"));
    settle(&mut [&mut me, &mut ada]);
    assert!(
        me.pieces().voices.is_empty(),
        "her microphone is not up yet"
    );

    relay.grant(MINTED, Some(ADA), huddle::seat());
    settle(&mut [&mut me, &mut ada]);
    assert_eq!(me.pieces().voices, ["mic/2"]);
    assert_eq!(me.pieces().pictures, ["cam/2"], "her picture");
}

// ---------- the door ----------

/// Ada's call, started alone: no one rung, its link is the way in.
fn adas_alone(line: &mut Line) {
    line.block(
        ADA,
        Op::Start {
            call: ADAS.into(),
            invited: vec![],
        },
    );
}

/// A link to a call you were not asked into is the door: the caller named,
/// Ask to join knocks and waits; let in, you join at once, floated as an
/// answer is.
#[test]
fn a_link_to_a_call_you_were_not_asked_into_is_the_door() {
    let mut line = line(false);
    adas_alone(&mut line);
    line.route.send(format!("call/{ADAS}"));
    line.cx.run_until_parked();
    assert!(
        line.cx.find("huddle-door").is_some(),
        "{:?}",
        line.cx.texts()
    );
    assert!(line.cx.has_text("ada is in a call"));
    assert!(line.sent.borrow().is_empty(), "nothing sent before the ask");
    assert_eq!(line.pieces().microphone, None, "no piece at the door");
    line.cx.simulate_click("huddle-ask");
    line.cx.run_until_parked();
    assert_eq!(
        line.sent.borrow().as_slice(),
        [Op::Knock { call: ADAS.into() }]
    );
    assert!(
        line.cx
            .has_text("Waiting for someone to let you in\u{2026}")
    );
    assert!(line.cx.find("huddle-ask").is_none(), "one knock at a time");
    line.block(
        ADA,
        Op::Admit {
            call: ADAS.into(),
            account: ME,
        },
    );
    assert_eq!(line.joins(), 1, "{:?}", line.sent.borrow());
    assert!(line.cx.find("huddle-bar").is_some(), "in the room");
    assert!(line.cx.find("huddle-door").is_none());
    assert_eq!(
        line.pieces().microphone,
        Some((track(ADAS, "mic/1".into()), false))
    );
}

/// Turned away, the door says so and Ask again knocks again; unanswered a
/// minute by the view's clock, it says no one answered; Back is the check.
#[test]
fn turned_away_or_unanswered_the_door_says_so_and_asks_again() {
    let mut line = line(false);
    adas_alone(&mut line);
    line.route.send(format!("call/{ADAS}"));
    line.cx.run_until_parked();
    line.cx.simulate_click("huddle-ask");
    line.cx.run_until_parked();
    line.block(
        ADA,
        Op::TurnAway {
            call: ADAS.into(),
            account: ME,
        },
    );
    assert!(
        line.cx.has_text("You were not let in."),
        "{:?}",
        line.cx.texts()
    );
    line.cx.simulate_click("huddle-ask");
    line.cx.run_until_parked();
    assert_eq!(
        line.sent.borrow().as_slice(),
        [
            Op::Knock { call: ADAS.into() },
            Op::Knock { call: ADAS.into() },
        ]
    );
    assert!(
        line.cx
            .has_text("Waiting for someone to let you in\u{2026}")
    );
    for _ in 0..huddle::KNOCK_SECONDS {
        line.second();
    }
    assert!(
        line.cx.has_text("No one answered."),
        "{:?}",
        line.cx.texts()
    );
    line.cx.simulate_click("huddle-door-leave");
    line.cx.run_until_parked();
    assert!(line.cx.find("huddle-join").is_some(), "on the check");
    assert_eq!(
        line.sent.borrow().len(),
        2,
        "a knock run out is not withdrawn"
    );
}

/// The link opened again after a turn away asks anew: what was answered
/// before is not said until you ask.
#[test]
fn a_door_opened_again_asks_anew() {
    let mut line = line(false);
    adas_alone(&mut line);
    line.route.send(format!("call/{ADAS}"));
    line.cx.run_until_parked();
    line.cx.simulate_click("huddle-ask");
    line.cx.run_until_parked();
    line.block(
        ADA,
        Op::TurnAway {
            call: ADAS.into(),
            account: ME,
        },
    );
    line.cx.simulate_click("huddle-door-leave");
    line.cx.run_until_parked();
    line.route.send(format!("call/{ADAS}"));
    line.cx.run_until_parked();
    line.wrote();
    assert!(
        line.cx.has_text("ada is in a call"),
        "{:?}",
        line.cx.texts()
    );
    assert!(!line.cx.has_text("You were not let in."));
}

/// Cancel while waiting withdraws the knock (Leave); Not now before asking
/// sends nothing.
#[test]
fn cancel_withdraws_the_knock_and_not_now_sends_nothing() {
    let mut line = line(false);
    adas_alone(&mut line);
    line.route.send(format!("call/{ADAS}"));
    line.cx.run_until_parked();
    line.cx.simulate_click("huddle-door-leave");
    line.cx.run_until_parked();
    assert!(line.cx.find("huddle-join").is_some(), "on the check");
    assert!(line.sent.borrow().is_empty());
    line.route.send(format!("call/{ADAS}"));
    line.cx.run_until_parked();
    line.cx.simulate_click("huddle-ask");
    line.cx.run_until_parked();
    line.cx.simulate_click("huddle-door-leave");
    line.cx.run_until_parked();
    assert_eq!(
        line.sent.borrow().as_slice(),
        [
            Op::Knock { call: ADAS.into() },
            Op::Leave { call: ADAS.into() },
        ]
    );
    assert!(line.cx.find("huddle-join").is_some(), "on the check");
}

/// Start a call in a room with no call starts one with no one rung; the
/// knocks at its door are cards over the stage, Let in admits, Turn away
/// turns away, and a knock that ran out goes with the next read.
#[test]
fn start_a_call_alone_and_answer_the_knocks_at_its_door() {
    let mut line = line(false);
    line.cx.simulate_click("huddle-join");
    line.cx.run_until_parked();
    assert!(line.cx.has_text("Start a call"), "{:?}", line.cx.texts());
    assert!(!line.cx.has_text("Copy link"), "no call to link yet");
    line.cx.simulate_click("huddle-start");
    line.cx.run_until_parked();
    assert_eq!(
        line.sent.borrow().as_slice(),
        [Op::Start {
            call: MINTED.into(),
            invited: vec![],
        }]
    );
    assert!(line.cx.has_text("Copy link"));
    assert!(!line.cx.has_text("Start a call"));
    assert!(
        line.cx
            .has_text("Only you are here. Invite someone, or copy the link.")
    );
    assert_eq!(
        line.pieces().microphone,
        Some((track(MINTED, "mic/1".into()), false))
    );
    line.block(
        BO,
        Op::Knock {
            call: MINTED.into(),
        },
    );
    line.block(
        ADA,
        Op::Knock {
            call: MINTED.into(),
        },
    );
    assert!(
        line.cx.has_text("bo is asking to join"),
        "{:?}",
        line.cx.texts()
    );
    assert!(line.cx.has_text("ada is asking to join"));
    line.cx.simulate_click(&format!("huddle-admit-{BO}"));
    line.cx.run_until_parked();
    assert_eq!(
        line.sent.borrow().last(),
        Some(&Op::Admit {
            call: MINTED.into(),
            account: BO,
        })
    );
    assert!(!line.cx.has_text("bo is asking to join"), "answered");
    line.cx.simulate_click(&format!("huddle-turn-away-{ADA}"));
    line.cx.run_until_parked();
    assert_eq!(
        line.sent.borrow().last(),
        Some(&Op::TurnAway {
            call: MINTED.into(),
            account: ADA,
        })
    );
    assert!(
        line.cx.find("huddle-knocks").is_none(),
        "{:?}",
        line.cx.texts()
    );
    // a third knock runs out by the chain's clock: the next read drops it
    line.block(
        5,
        Op::Knock {
            call: MINTED.into(),
        },
    );
    assert!(line.cx.has_text("account 5 is asking to join"));
    line.now.set(1_000 + huddle::KNOCK_SECONDS);
    for _ in 0..5 {
        line.second();
    }
    assert!(
        line.cx.find("huddle-knocks").is_none(),
        "{:?}",
        line.cx.texts()
    );
}

/// Alone in a call, the view says you are still there every half wait,
/// so its link keeps working past the first; with someone else in it, it
/// says nothing.
#[test]
fn alone_in_a_call_you_keep_it_waiting_for_knocks() {
    let mut line = line(false);
    line.cx.simulate_click("huddle-join");
    line.cx.run_until_parked();
    line.cx.simulate_click("huddle-start");
    line.cx.run_until_parked();
    let joins = line.joins();
    line.now.set(1_000 + huddle::WAIT_SECONDS / 2);
    for _ in 0..huddle::WAIT_SECONDS / 2 {
        line.second();
    }
    assert_eq!(line.joins(), joins + 1, "{:?}", line.sent.borrow());
    // past the wait from the start: the door is open still
    line.now.set(1_000 + huddle::WAIT_SECONDS);
    line.block(
        BO,
        Op::Knock {
            call: MINTED.into(),
        },
    );
    let tracks = line.tracks();
    line.arrives(&tracks, ADA);
    for _ in 0..huddle::WAIT_SECONDS / 2 {
        line.second();
    }
    assert_eq!(line.joins(), joins + 1, "not alone: nothing to keep");
}

/// At one call's door, a link to another withdraws the knock at the
/// first.
#[test]
fn a_door_left_for_another_withdraws_its_knock() {
    let mut line = line(false);
    adas_alone(&mut line);
    line.block(
        BO,
        Op::Start {
            call: "call-bo".into(),
            invited: vec![],
        },
    );
    line.route.send(format!("call/{ADAS}"));
    line.cx.run_until_parked();
    line.cx.simulate_click("huddle-ask");
    line.cx.run_until_parked();
    line.route.send("call/call-bo".into());
    line.cx.run_until_parked();
    assert_eq!(
        line.sent.borrow().as_slice(),
        [
            Op::Knock { call: ADAS.into() },
            Op::Leave { call: ADAS.into() },
        ]
    );
    assert!(line.cx.has_text("bo is in a call"), "{:?}", line.cx.texts());
}

/// At one call's door, answering another you were asked into withdraws
/// the knock at the first.
#[test]
fn answering_a_call_from_a_door_withdraws_its_knock() {
    let mut line = line(false);
    adas_alone(&mut line);
    line.route.send(format!("call/{ADAS}"));
    line.cx.run_until_parked();
    line.cx.simulate_click("huddle-ask");
    line.cx.run_until_parked();
    line.block(
        BO,
        Op::Start {
            call: "call-bo".into(),
            invited: vec![ME],
        },
    );
    line.route.send("call/call-bo".into());
    line.cx.run_until_parked();
    assert_eq!(
        line.sent.borrow().as_slice(),
        [
            Op::Knock { call: ADAS.into() },
            Op::Leave { call: ADAS.into() },
            Op::Join {
                call: "call-bo".into()
            },
        ]
    );
    assert!(line.cx.find("huddle-bar").is_some(), "in Bo's call");
}

/// Copy link puts the call's link on the clipboard and says so for a
/// while.
#[test]
fn copy_link_puts_the_calls_link_on_the_clipboard() {
    let mut line = line(false);
    let copied = Rc::new(RefCell::new(String::new()));
    let seen = copied.clone();
    line.cx
        .host()
        .handle::<view::methods::ClipboardWrite>(move |text| {
            *seen.borrow_mut() = text;
            Ok(())
        });
    line.cx.simulate_click("huddle-join");
    line.cx.simulate_click("huddle-start");
    line.cx.run_until_parked();
    line.cx.simulate_click("huddle-copy-link");
    line.cx.run_until_parked();
    assert_eq!(
        *copied.borrow(),
        format!("duck://lab-0a1b2c3d/huddle/call/{MINTED}")
    );
    assert!(line.cx.has_text("Link copied"), "{:?}", line.cx.texts());
    for _ in 0..5 {
        line.second();
    }
    assert!(line.cx.has_text("Copy link"));
}

/// On a phone the door is the whole screen, and the room's knocks a strip
/// over the stage, with Start a call and Copy link in the title row.
#[test]
fn on_a_phone_the_door_is_the_whole_screen_and_knocks_a_strip() {
    let mut line = line(true);
    adas_alone(&mut line);
    line.route.send(format!("call/{ADAS}"));
    line.cx.run_until_parked();
    assert!(
        line.cx.find("huddle-door").is_some(),
        "{:?}",
        line.cx.texts()
    );
    assert!(line.cx.has_text("ada is in a call"));
    assert!(
        line.cx.find("huddle-join").is_none(),
        "the check is under it"
    );
    line.cx.simulate_click("huddle-ask");
    line.cx.run_until_parked();
    assert!(
        line.cx
            .has_text("Waiting for someone to let you in\u{2026}")
    );
    line.block(
        ADA,
        Op::Admit {
            call: ADAS.into(),
            account: ME,
        },
    );
    assert_eq!(line.joins(), 1);
    assert!(line.cx.find("huddle-leave").is_some(), "in the room");
    assert_eq!(
        line.cx
            .interactivity("huddle-copy-link")
            .aria
            .label
            .as_deref(),
        Some("Copy link")
    );
    // a knock at this call's door, which Ada started and I am now in
    line.block(BO, Op::Knock { call: ADAS.into() });
    assert!(
        line.cx.has_text("bo is asking to join"),
        "{:?}",
        line.cx.texts()
    );
    line.cx.simulate_click(&format!("huddle-admit-{BO}"));
    line.cx.run_until_parked();
    assert_eq!(
        line.sent.borrow().last(),
        Some(&Op::Admit {
            call: ADAS.into(),
            account: BO,
        })
    );
    assert!(line.cx.find("huddle-knocks").is_none());
    // out, and a call started alone from the title row
    line.cx.simulate_click("huddle-leave");
    line.cx.run_until_parked();
    line.cx.simulate_click("huddle-join");
    line.cx.run_until_parked();
    assert_eq!(
        line.cx.interactivity("huddle-start").aria.label.as_deref(),
        Some("Start a call")
    );
    line.cx.simulate_click("huddle-start");
    line.cx.run_until_parked();
    assert_eq!(
        line.sent.borrow().last(),
        Some(&Op::Start {
            call: MINTED.into(),
            invited: vec![],
        })
    );
}
