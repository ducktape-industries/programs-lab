//! The call the room is in: a room of the `huddle` program, from the Start
//! or the Join that put the person in it to the Leave that takes them out,
//! and the calls ringing them.
//!
//! Who is in a call is its room's live tracks (`live.tracks`): an account
//! is in while its microphone track is up, muted or not, with its camera's
//! up or not. The program's `joined` lags them by a block and never hears a
//! crash, so the stage never reads it. The view places your camera and
//! microphone publishing into your own tracks (`cam/<you>`, `mic/<you>`),
//! and for each other person a voice (every one in the call: the stage
//! does not decide who is heard) and, on the stage, a picture.
//!
//! The node holds a track the program has not granted yet and says
//! `Waiting`; the view then sends `Join`, which writes the person's row,
//! once per `Waiting` (arcade's versus asks the same way): Join is
//! idempotent, so a Join the row was on its way for anyway costs one op.
//!
//! The door: a link to a call opened by someone not asked in
//! ([`Huddle::arrive`]) stands them at its door ([`AtTheDoor`]), where Ask
//! to join sends `Knock`; the program's blocks then say whether they were
//! let in (asked in: they join as an invitee does) or turned away, and the
//! view's own clock says when no one answered ([`huddle::KNOCK_SECONDS`]).
//! In a call, the knocks waiting at its door are read with the call and
//! asked again every five seconds while any waits (a knock runs out by the
//! chain's clock, with no block to say so); Let in and Turn away answer
//! them.
//!
//! The main tile is the last person whose voice said they speak, held two
//! seconds after they stop so two people trading words do not flip it each
//! word; before anyone speaks, the first other person. Never you with others
//! there.
//!
//! The calls ringing the person are the program's `Mine`, asked again on
//! each block that writes one and, while one is shown, every five seconds:
//! a ring runs out by the chain's clock, with no block to say so.
use std::collections::{BTreeMap, BTreeSet};

use huddle::{KNOCK_SECONDS, Listed, Op, Standing};
use identity::{Identity, Kind as Account, Profile};
use view::methods::{ClipboardWrite, LiveAnnounce, Submitted, Watching};
use view::prelude::*;

use crate::{DoorStage, Huddle, Screen, words};

/// How long the main tile stays with whoever took it before the next one
/// to start speaking takes it, in the view's seconds.
const SPEAKER_HOLD: u64 = 2;

/// How long a person's track may stay `Starting` before the room says it
/// is reconnecting, in the view's seconds.
const SLOW_START: u64 = 2;

/// How often a shown ring is asked again, in the view's seconds.
const RING_CHECK: u64 = 5;

/// How long the room says why an op of the call did not go through, in the
/// view's seconds: the call goes on, and the line is the network's again.
pub(crate) const NOTE_SECONDS: u64 = 5;

/// The most people the desktop's stage shows; the rest are a "+N" chip.
pub(crate) const STAGE: usize = 6;

/// The call the room is in.
pub(crate) struct Call {
    pub(crate) id: String,
    /// Everyone else in the room, by account: in while their microphone
    /// track is up.
    pub(crate) people: BTreeMap<u64, Person>,
    /// Everyone whose camera's track is up, in the room or not yet: the
    /// room lists tracks in no set order, so a camera heard before its
    /// microphone still shows once its person is in.
    pub(crate) cameras: BTreeSet<u64>,
    /// Who waits at the call's door, as the program last listed them.
    pub(crate) knocks: Vec<u64>,
    /// The room's track list, followed while in the call.
    _watch: Task<()>,
    /// A Join went out for the `Waiting` heard; cleared when a track is up
    /// or the Join is refused, so each `Waiting` sends one.
    row_asked: bool,
    /// Who holds the main tile, and since when.
    speaker: Option<(u64, u64)>,
    /// The last person whose voice turned to speaking: the main tile's
    /// next holder once the hold is out.
    last_spoke: Option<u64>,
}

/// One other person in the call.
#[derive(Default)]
pub(crate) struct Person {
    /// What their voice's piece last told: whether they speak, and how its
    /// track stands.
    pub(crate) voice: MediaState,
    /// What their picture's piece last told.
    pub(crate) picture: MediaState,
    /// When one of their pieces began waiting for its first objects, by
    /// the view's seconds; `None` while none waits.
    starting_since: Option<u64>,
}

/// You at the door of a call you were not asked into: which, whose, and
/// where you stand, since when by the view's clock.
pub(crate) struct AtTheDoor {
    pub(crate) id: String,
    pub(crate) caller: u64,
    pub(crate) stage: DoorStage,
    since: u64,
}

/// Which of a person's pieces told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Piece {
    Voice,
    Picture,
}

impl Person {
    /// Starts or stops the clock of a piece waiting: a piece placed anew
    /// waits (the host tells a state only when it moves off `Starting`),
    /// and a picture is drawn only while their `camera` is up.
    fn clock(&mut self, now: u64, camera: bool) {
        let starting = self.voice.status == MediaStatus::Starting
            || (camera && self.picture.status == MediaStatus::Starting);
        self.starting_since = match (starting, self.starting_since) {
            (true, None) => Some(now),
            (true, since) => since,
            (false, _) => None,
        };
    }
}

impl Call {
    fn new(id: String, watch: Task<()>) -> Self {
        Self {
            id,
            people: BTreeMap::new(),
            cameras: BTreeSet::new(),
            knocks: Vec::new(),
            _watch: watch,
            row_asked: false,
            speaker: None,
            last_spoke: None,
        }
    }

    /// The person on the main tile: the speaker while they hold it, else
    /// the first other person; `None` alone.
    pub(crate) fn main(&self) -> Option<u64> {
        self.speaker
            .map(|(account, _)| account)
            .filter(|account| self.people.contains_key(account))
            .or_else(|| self.people.keys().next().copied())
    }

    /// Everyone else in stage order: the main tile's first, the rest by
    /// number.
    pub(crate) fn order(&self) -> Vec<u64> {
        let main = self.main();
        main.into_iter()
            .chain(self.people.keys().copied().filter(|n| Some(*n) != main))
            .collect()
    }

    /// Who is in the call, you among them.
    pub(crate) fn count(&self) -> usize {
        self.people.len() + 1
    }

    /// The main tile handed on, at `now`: to the last person whose voice
    /// turned to speaking, once its holder has had it [`SPEAKER_HOLD`]
    /// (at once when its holder left). No one speaking since, it stays.
    fn hand_on(&mut self, now: u64) {
        let held = self.speaker.is_some_and(|(account, since)| {
            self.people.contains_key(&account) && now < since + SPEAKER_HOLD
        });
        if held {
            return;
        }
        if let Some(last) = self.last_spoke.filter(|n| self.people.contains_key(n))
            && self.speaker.map(|(account, _)| account) != Some(last)
        {
            self.speaker = Some((last, now));
        }
    }

    /// Whether a person's track has waited longer than [`SLOW_START`].
    pub(crate) fn slow(&self, now: u64) -> bool {
        self.people.values().any(|person| {
            person
                .starting_since
                .is_some_and(|since| now >= since + SLOW_START)
        })
    }
}

impl Huddle {
    /// Follows what calls need for as long as the view runs: the session
    /// (who you are), the program's blocks (the calls ringing you) and the
    /// links opened into the view ([`Huddle::opened`]).
    pub(crate) fn watch_calls(&mut self, cx: &mut Context<Self>) {
        cx.follow::<HostSession>((), |view, session, cx| match session {
            Ok(next) => view.session_changed(next, cx),
            Err(refusal) => cx.log_refused("the session", &refusal),
        })
        .detach();
        cx.follow::<Changes<huddle::Huddle>>((), |view, change, cx| match change {
            Ok(change) => {
                let Some(account) = view.session.account else {
                    return;
                };
                let knocks_at = |id: &str| huddle::ask::Knocks {
                    call: id.to_owned(),
                };
                let ringing = huddle::ask::Ringing { account };
                if touches(&change, &ringing) {
                    view.read_calls(cx);
                }
                if let Some(call) = &view.call
                    && touches(&change, &knocks_at(&call.id))
                {
                    view.read_knocks(cx);
                }
                if let Some(door) = &view.door
                    && touches(&change, &knocks_at(&door.id))
                {
                    view.check_door(cx);
                }
            }
            Err(refusal) => cx.log_refused("huddle's blocks", &refusal),
        })
        .detach();
        cx.follow::<HostRoute>((), |_, route, cx| match route {
            Ok(route) => {
                // after this update, where the window says what the pointer
                // is; each in the order the links were opened, so an end
                // right behind its answer finds the call answered
                cx.spawn(async move |view, cx| {
                    let _ = view.update_in(cx, |view, window, cx| {
                        view.opened(&route, window.touch(), cx)
                    });
                })
                .detach();
            }
            Err(refusal) => cx.log_refused("the links opened here", &refusal),
        })
        .detach();
    }

    /// A link opened into the view, by its route: `call/<id>` (a ring's
    /// link, or one shared) answers call `id` when you were asked in, else
    /// stands you at its door; `call/<id>/leave` (a ring's end link: the
    /// person ended the answered call from the system's call screen) leaves
    /// it, and only it. Any other route opens nothing.
    fn opened(&mut self, route: &str, touch: bool, cx: &mut Context<Self>) {
        match route.split('/').collect::<Vec<_>>().as_slice() {
            ["call", id] => self.arrive((*id).to_owned(), touch, cx),
            ["call", id, "leave"] => self.leave_call(id, cx),
            _ => {}
        }
    }

    /// At call `id` by its link: in the call already, nothing; asked in,
    /// answered; not asked, at its door, out of any other call and on the
    /// check; over or no such call, said on the check. The call is read
    /// first, so an end link right behind ([`Huddle::leave_call`]) finds
    /// the arrival on its way and calls it off.
    fn arrive(&mut self, id: String, touch: bool, cx: &mut Context<Self>) {
        if self.call.as_ref().is_some_and(|call| call.id == id) {
            return;
        }
        let Some(me) = self.session.account else {
            return;
        };
        self.arriving = Some(id.clone());
        let asked = cx.host().query(huddle::ask::Call { id: id.clone() });
        cx.land(asked, move |view, call, cx| {
            // called off, or a later link's arrival is the one on its way
            if view.arriving.as_deref() != Some(id.as_str()) {
                return;
            }
            view.arriving = None;
            let call = match call {
                Ok(call) => call,
                Err(refusal) => return view.call_failed(&refusal.message, cx),
            };
            match call {
                None => view.said_on_the_check(words::NO_SUCH_CALL.into(), cx),
                Some(call) if call.ended.is_some() => {
                    view.said_on_the_check(words::CALL_ENDED.into(), cx)
                }
                Some(call) if call.asked(me) => view.answer(id, touch, cx),
                Some(call) => {
                    if view.screen == Screen::Room {
                        view.leave(cx);
                    }
                    view.ask_names(vec![call.caller], cx);
                    view.door = Some(AtTheDoor {
                        id,
                        caller: call.caller,
                        stage: DoorStage::Asking,
                        since: view.ticks,
                    });
                    cx.notify();
                }
            }
        })
        .detach();
    }

    /// `note` said on the check, out of the room.
    fn said_on_the_check(&mut self, note: String, cx: &mut Context<Self>) {
        if self.screen == Screen::Room {
            self.leave(cx);
        }
        self.say(note);
        cx.notify();
    }

    /// Ask to join: your knock at the door, waiting from now.
    pub(crate) fn ask_to_join(&mut self, cx: &mut Context<Self>) {
        let Some(door) = &mut self.door else { return };
        door.stage = DoorStage::Waiting;
        door.since = self.ticks;
        self.call_note = None;
        let id = door.id.clone();
        let knocked = cx
            .host()
            .ask::<Submit<huddle::Huddle>>(Op::Knock { call: id.clone() });
        cx.land(knocked, move |view, knocked, cx| {
            let Some(why) = refused(knocked) else { return };
            if let Some(door) = view.door.as_mut().filter(|door| door.id == id) {
                door.stage = DoorStage::Asking;
            }
            view.call_failed(&why, cx);
        })
        .detach();
        cx.notify();
    }

    /// Away from the door, back to the check: a knock still waiting is
    /// withdrawn.
    pub(crate) fn leave_door(&mut self, cx: &mut Context<Self>) {
        if let Some(door) = self.door.take() {
            if door.stage == DoorStage::Waiting {
                self.send(Op::Leave { call: door.id }, cx);
            }
            self.call_note = None;
            cx.notify();
        }
    }

    /// A block wrote the door's call: let in, you join (as a phone or a
    /// desktop does); turned away or run out, the door says so; the call
    /// gone, the check does.
    fn check_door(&mut self, cx: &mut Context<Self>) {
        let (Some(door), Some(me)) = (&self.door, self.session.account) else {
            return;
        };
        let id = door.id.clone();
        let host = cx.host();
        let asked = async move {
            let call = host.query(huddle::ask::Call { id: id.clone() }).await?;
            let knocks = host.query(huddle::ask::Knocks { call: id }).await?;
            Ok::<_, Error>((call, knocks))
        };
        cx.spawn(async move |view, cx| {
            let answered = asked.await;
            let _ = view.update_in(cx, |view, window, cx| {
                let Some(door) = &view.door else { return };
                let (call, knocks) = match answered {
                    Ok(answered) => answered,
                    Err(refusal) => return view.call_failed(&refusal.message, cx),
                };
                let id = door.id.clone();
                match call {
                    None | Some(huddle::Call { ended: Some(_), .. }) => {
                        view.door = None;
                        view.say(words::CALL_ENDED.into());
                    }
                    Some(call) if call.asked(me) => {
                        view.door = None;
                        view.answer(id, window.touch(), cx);
                    }
                    Some(_) => {
                        let standing = knocks
                            .into_iter()
                            .flatten()
                            .find(|knocking| knocking.account == me)
                            .map(|knocking| knocking.standing);
                        let stage = match standing {
                            Some(Standing::TurnedAway) => Some(DoorStage::TurnedAway),
                            Some(Standing::RanOut) => Some(DoorStage::NoAnswer),
                            _ => None,
                        };
                        if let (Some(stage), Some(door)) = (stage, &mut view.door) {
                            door.stage = stage;
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn session_changed(&mut self, next: Session, cx: &mut Context<Self>) {
        if next == self.session {
            return;
        }
        let prev = std::mem::replace(&mut self.session, next);
        if self.session.account != prev.account {
            // another account's calls: the one under way is not theirs
            self.call = None;
            self.door = None;
            self.ringing = Loadable::Idle;
            self.invitable = Loadable::Idle;
            self.read_calls(cx);
        } else if self.session.connected && !prev.connected {
            // a reconnect hears `Waiting` again, and Joins again on it
            if let Some(call) = &mut self.call {
                call.row_asked = false;
            }
            self.read_calls(cx);
        }
        cx.notify();
    }

    /// The calls ringing you, as the program lists them at the chain's
    /// time.
    pub(crate) fn read_calls(&mut self, cx: &mut Context<Self>) {
        let Some(account) = self.session.account else {
            return;
        };
        let ringing = cx.host().query(huddle::ask::Ringing { account });
        cx.land(ringing, |view, ringing, cx| {
            if let Ok(calls) = &ringing {
                let callers = calls.iter().map(|listed| listed.call.caller).collect();
                view.ask_names(callers, cx);
            }
            view.ringing = ringing.into();
            cx.notify();
        })
        .detach();
    }

    /// The calls ringing you, the one the room is in aside.
    pub(crate) fn rings(&self) -> Vec<&Listed> {
        let Some(ringing) = self.ringing.ready() else {
            return Vec::new();
        };
        let here = self.call.as_ref().map(|call| call.id.as_str());
        ringing
            .iter()
            .filter(|listed| Some(listed.id.as_str()) != here)
            .collect()
    }

    /// One second of the view's clock in a call: the main tile handed on,
    /// a shown ring and a waiting knock asked again every [`RING_CHECK`],
    /// your own knock unanswered a minute said so, Copy link's word back.
    pub(crate) fn call_tick(&mut self, cx: &mut Context<Self>) {
        let now = self.ticks;
        if let Some(call) = &mut self.call {
            call.hand_on(now);
        }
        if self.screen == Screen::Room && now >= self.noted + NOTE_SECONDS {
            self.call_note = None;
        }
        if self.copied.is_some_and(|at| now >= at + NOTE_SECONDS) {
            self.copied = None;
            cx.notify();
        }
        if !self.rings().is_empty() && now.is_multiple_of(RING_CHECK) {
            self.read_calls(cx);
        }
        if self
            .call
            .as_ref()
            .is_some_and(|call| !call.knocks.is_empty())
            && now.is_multiple_of(RING_CHECK)
        {
            self.read_knocks(cx);
        }
        if let Some(door) = &mut self.door
            && door.stage == DoorStage::Waiting
            && now >= door.since + KNOCK_SECONDS
        {
            door.stage = DoorStage::NoAnswer;
            cx.notify();
        }
    }

    /// Who waits at the room's door, as the program lists them at the
    /// chain's time.
    pub(crate) fn read_knocks(&mut self, cx: &mut Context<Self>) {
        let Some(call) = &self.call else { return };
        let id = call.id.clone();
        let knocks = cx.host().query(huddle::ask::Knocks { call: id.clone() });
        cx.land(knocks, move |view, knocks, cx| {
            let waiting: Vec<u64> = match knocks {
                Ok(Some(knocks)) => knocks
                    .into_iter()
                    .filter(|knocking| knocking.standing == Standing::Waiting)
                    .map(|knocking| knocking.account)
                    .collect(),
                Ok(None) => Vec::new(),
                Err(refusal) => return cx.log_refused("the door", &refusal),
            };
            view.ask_names(waiting.clone(), cx);
            if let Some(call) = view.call.as_mut().filter(|call| call.id == id) {
                call.knocks = waiting;
                cx.notify();
            }
        })
        .detach();
    }

    /// Let `account`, knocking, in: asked in, their view joins at once.
    pub(crate) fn admit(&mut self, account: u64, cx: &mut Context<Self>) {
        self.answer_knock(account, |call, account| Op::Admit { call, account }, cx);
    }

    /// Turn `account`'s knock away.
    pub(crate) fn turn_away(&mut self, account: u64, cx: &mut Context<Self>) {
        self.answer_knock(account, |call, account| Op::TurnAway { call, account }, cx);
    }

    /// `answer` sent for `account`'s knock, which leaves the door at once;
    /// a refusal brings it back with the next read.
    fn answer_knock(
        &mut self,
        account: u64,
        answer: fn(String, u64) -> Op,
        cx: &mut Context<Self>,
    ) {
        let Some(call) = &mut self.call else { return };
        call.knocks.retain(|knocker| *knocker != account);
        let op = answer(call.id.clone(), account);
        self.send(op, cx);
        cx.notify();
    }

    /// The call's link (`duck://<chain>/huddle/call/<id>`) onto the
    /// clipboard; Copy link says so for [`NOTE_SECONDS`].
    pub(crate) fn copy_link(&mut self, cx: &mut Context<Self>) {
        let Some(link) = self.call_link() else { return };
        let copied = cx.host().ask::<ClipboardWrite>(link);
        cx.land(copied, |view, copied, cx| {
            match copied {
                Ok(()) => view.copied = Some(view.ticks),
                Err(refusal) => view.say(words::call_trouble(&refusal.message)),
            }
            cx.notify();
        })
        .detach();
    }

    /// The link to the call the room is in, as a ring's notice carries it.
    pub(crate) fn call_link(&self) -> Option<String> {
        let call = self.call.as_ref()?;
        ducklink::mint(&self.session.chain_id, huddle::PROGRAM, &["call", &call.id])
    }

    /// Whether Copy link still says the link was copied.
    pub(crate) fn just_copied(&self) -> bool {
        self.copied.is_some()
    }

    /// What identity calls `account`, or its number until it answers.
    pub(crate) fn name(&self, account: u64) -> String {
        self.names
            .get(&account)
            .cloned()
            .unwrap_or_else(|| words::account(account))
    }

    /// Asks identity for each of `accounts` not named yet.
    pub(crate) fn ask_names(&mut self, accounts: Vec<u64>, cx: &mut Context<Self>) {
        for account in accounts {
            if self.names.contains_key(&account) {
                continue;
            }
            // asked once: the number stands in until the answer lands
            self.names.insert(account, words::account(account));
            let profile = cx
                .host()
                .ask::<Query<Identity>>(::identity::Query::Profile { number: account });
            cx.land(profile, move |view, profile, cx| {
                if let Ok(::identity::Reply::Profile(Some(profile))) = profile {
                    view.names.insert(account, profile.name);
                    cx.notify();
                }
            })
            .detach();
        }
    }

    /// Everyone you can invite: identity's people, you aside, read anew
    /// whenever the list opens.
    pub(crate) fn read_invitable(&mut self, cx: &mut Context<Self>) {
        let me = self.session.account;
        let host = cx.host();
        let people = async move {
            let mut people = Vec::new();
            let mut after = None;
            loop {
                let (profiles, next) = host
                    .query(::identity::ask::Profiles { after, limit: 100 })
                    .await?;
                people.extend(
                    profiles
                        .into_iter()
                        .filter(|profile| profile.kind == Account::User)
                        .filter(|profile| Some(profile.number) != me),
                );
                let Some(next) = next else { break };
                after = Some(next);
            }
            Ok::<Vec<Profile>, Error>(people)
        };
        cx.load(self, people, |view| &mut view.invitable);
    }

    /// Rings `account`: into the call under way, or a new call with you in
    /// it (its id minted by the host) when the room has none.
    pub(crate) fn invite(&mut self, account: u64, cx: &mut Context<Self>) {
        self.call_note = None;
        self.ask_names(vec![account], cx);
        if let Some(call) = &self.call {
            let op = Op::Invite {
                call: call.id.clone(),
                accounts: vec![account],
            };
            self.send(op, cx);
            return;
        }
        self.start(vec![account], cx);
    }

    /// Start a call: a new call with you alone in it, waiting for knocks
    /// at the link Copy link gives.
    pub(crate) fn start_alone(&mut self, cx: &mut Context<Self>) {
        self.call_note = None;
        if self.call.is_none() {
            self.start(Vec::new(), cx);
        }
    }

    /// A new call with you in it, its id minted by the host, ringing
    /// `invited`.
    fn start(&mut self, invited: Vec<u64>, cx: &mut Context<Self>) {
        let minted = cx.host().ask::<HostId>("call".into());
        cx.land(minted, move |view, id, cx| {
            let id = match id {
                Ok(id) => id,
                Err(refusal) => return view.call_failed(&refusal.message, cx),
            };
            let op = Op::Start {
                call: id.clone(),
                invited,
            };
            let started = cx.host().ask::<Submit<huddle::Huddle>>(op);
            cx.land(started, move |view, started, cx| match refused(started) {
                // still in the room it was started from
                None if view.screen == Screen::Room && view.call.is_none() => {
                    view.enter_call(id, cx)
                }
                // left, or in another call, before it landed: out of it,
                // so no one is rung into an empty room
                None => view.send(Op::Leave { call: id }, cx),
                Some(why) => view.call_failed(&why, cx),
            })
            .detach();
        })
        .detach();
    }

    /// Answers call `id`: in the room at once (on a phone, `touch`, where
    /// the view is; else floated), the microphone on (a phone that rang
    /// wants the call, not a settings page), the Join on its way.
    pub(crate) fn answer(&mut self, id: String, touch: bool, cx: &mut Context<Self>) {
        if self.call.as_ref().is_some_and(|call| call.id == id) {
            return;
        }
        self.hang_up(cx);
        self.door = None;
        self.call_note = None;
        self.muted = false;
        self.enter_call(id, cx);
        self.join_row(cx);
        match self.screen {
            Screen::Room => {}
            Screen::Check if touch => self.enter_room(cx),
            Screen::Check => self.join(cx),
        }
    }

    /// Says no to call `id`: its ring stops here at once, and for the
    /// program on the next block.
    pub(crate) fn decline(&mut self, id: String, cx: &mut Context<Self>) {
        if let Some(ringing) = self.ringing.ready_mut() {
            ringing.retain(|listed| listed.id != id);
        }
        self.send(Op::Decline { call: id }, cx);
        cx.notify();
    }

    /// Out of call `id` as Leave takes the person out, when the room is in
    /// it; an arrival at it still on its way is called off. In another
    /// call, or none (a link that opens the view finds none), nothing: the
    /// end of a call the room has left already, landing late, must not end
    /// the call the person is in now, and with no call the room publishes
    /// nothing, so no one hears or sees them.
    fn leave_call(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.arriving.as_deref() == Some(id) {
            self.arriving = None;
        }
        if self.call.as_ref().is_some_and(|call| call.id == id) {
            self.leave(cx);
        }
    }

    /// Out of the call: the pieces leave the frame with it, and the
    /// program hears `Leave`.
    pub(crate) fn hang_up(&mut self, cx: &mut Context<Self>) {
        if let Some(call) = self.call.take() {
            self.send(Op::Leave { call: call.id }, cx);
        }
    }

    /// Into call `id`: its tracks followed, its door read; the pieces
    /// publish from the next frame.
    fn enter_call(&mut self, id: String, cx: &mut Context<Self>) {
        let watch = cx.follow::<LiveTracks>(id.clone(), |view, item, cx| match item {
            Ok(Watching::Waiting) => view.join_row(cx),
            Ok(Watching::Announce(announce)) => view.announced(announce, cx),
            Err(refusal) => view.call_over(&refusal, cx),
        });
        self.call = Some(Call::new(id, watch));
        self.copied = None;
        self.read_knocks(cx);
        let title = words::in_the_room(1);
        cx.host().notify::<HostTitle>(title);
        cx.notify();
    }

    /// The program has not granted a track of the call yet: Join, unless
    /// one is out for this `Waiting`.
    pub(crate) fn join_row(&mut self, cx: &mut Context<Self>) {
        let Some(call) = &mut self.call else { return };
        if std::mem::replace(&mut call.row_asked, true) {
            return;
        }
        let id = call.id.clone();
        let joined = cx
            .host()
            .ask::<Submit<huddle::Huddle>>(Op::Join { call: id.clone() });
        cx.land(joined, move |view, joined, cx| {
            let Some(why) = refused(joined) else { return };
            let Some(call) = view.call.as_mut().filter(|call| call.id == id) else {
                return;
            };
            // the next `Waiting` asks again
            call.row_asked = false;
            view.call_failed(&why, cx);
        })
        .detach();
    }

    /// A track of the call came up or went down: a microphone's brings its
    /// person in or out, a camera's their picture.
    fn announced(&mut self, announce: LiveAnnounce, cx: &mut Context<Self>) {
        let me = self.session.account;
        let Some(call) = &mut self.call else { return };
        let who = announce.by;
        if Some(who) == me {
            return;
        }
        if announce.name == huddle::microphone_track(who) {
            match announce.up {
                true => {
                    let camera = call.cameras.contains(&who);
                    call.people
                        .entry(who)
                        .or_default()
                        .clock(self.ticks, camera);
                }
                false => {
                    call.people.remove(&who);
                }
            }
            call.hand_on(self.ticks);
            let count = call.count();
            cx.host().notify::<HostTitle>(words::in_the_room(count));
            self.ask_names(vec![who], cx);
        } else if announce.name == huddle::camera_track(who) {
            match announce.up {
                true => call.cameras.insert(who),
                false => call.cameras.remove(&who),
            };
            if let Some(person) = call.people.get_mut(&who) {
                // a picture placed anew starts over
                person.picture = MediaState::default();
                person.clock(self.ticks, announce.up);
            }
        }
        cx.notify();
    }

    /// One of your own publishing pieces told how its track stands:
    /// `Waiting` asks for the row, `Up` clears the ask for the next one.
    pub(crate) fn heard_own_track(&mut self, track: &TrackState, cx: &mut Context<Self>) {
        match track {
            TrackState::Waiting => self.join_row(cx),
            TrackState::Up { .. } => {
                if let Some(call) = &mut self.call {
                    call.row_asked = false;
                }
            }
            TrackState::None | TrackState::Ended { .. } => {}
        }
    }

    /// A piece of `who`'s told its state: a voice's speaking hands the main
    /// tile on; a piece waiting for its first objects starts the clock the
    /// network row reads.
    pub(crate) fn heard_person(
        &mut self,
        who: u64,
        piece: Piece,
        state: &MediaState,
        cx: &mut Context<Self>,
    ) {
        let now = self.ticks;
        let Some(call) = &mut self.call else { return };
        let camera = call.cameras.contains(&who);
        let Some(person) = call.people.get_mut(&who) else {
            return;
        };
        let was_speaking = person.voice.speaking;
        match piece {
            Piece::Voice => person.voice = state.clone(),
            Piece::Picture => person.picture = state.clone(),
        }
        person.clock(now, camera);
        if piece == Piece::Voice && !was_speaking && state.speaking {
            call.last_spoke = Some(who);
        }
        call.hand_on(now);
        cx.notify();
    }

    /// Whether the room says it is reconnecting: a track of yours not up,
    /// or someone's slow to come through.
    pub(crate) fn reconnecting(&self) -> bool {
        let Some(call) = &self.call else { return false };
        let down = |state: &MediaState| {
            matches!(state.track, TrackState::Waiting | TrackState::Ended { .. })
        };
        down(&self.mic_state)
            || (self.camera_placed() && down(&self.camera_state))
            || call.slow(self.ticks)
    }

    /// Whether the room's line says the note: there is one, and nothing is
    /// reconnecting, which wins while it holds.
    pub(crate) fn noting(&self) -> bool {
        self.call_note.is_some() && !self.reconnecting()
    }

    /// What the room's line (the network row) says, if anything:
    /// reconnecting, why the call's last op did not go through, or the
    /// camera sending less picture (the host's words).
    pub(crate) fn network(&self) -> Option<String> {
        if self.noting() {
            return self.call_note.clone();
        }
        if self.reconnecting() {
            return Some(words::RECONNECTING.into());
        }
        let camera = &self.camera_state;
        (self.call.is_some()
            && self.camera_placed()
            && camera.status == MediaStatus::Live
            && !camera.detail.is_empty())
        .then(|| camera.detail.clone())
    }

    /// Who is in the room, you among them.
    pub(crate) fn people_here(&self) -> usize {
        self.call.as_ref().map_or(1, Call::count)
    }

    /// The call's tracks were refused for good: the grant taken back (the
    /// caller ended it, or it was over: nothing to leave), or no live
    /// connection (left, so no one waits on you). Out of the room and back
    /// on the desk, with why.
    fn call_over(&mut self, refusal: &Error, cx: &mut Context<Self>) {
        let note = match refusal.code.as_str() {
            "not_granted" => {
                self.call = None;
                words::CALL_ENDED.into()
            }
            _ => words::call_trouble(&refusal.message),
        };
        self.leave(cx);
        self.say(note);
    }

    /// An op of the call refused: said.
    fn call_failed(&mut self, why: &str, cx: &mut Context<Self>) {
        self.say(words::call_trouble(why));
        cx.notify();
    }

    /// `note` said from now.
    fn say(&mut self, note: String) {
        self.call_note = Some(note);
        self.noted = self.ticks;
    }

    /// Sends `op`; a refusal is said in the room.
    fn send(&mut self, op: Op, cx: &mut Context<Self>) {
        let sent = cx.host().ask::<Submit<huddle::Huddle>>(op);
        cx.land(sent, |view, sent, cx| {
            if let Some(why) = refused(sent) {
                view.call_failed(&why, cx);
            }
        })
        .detach();
    }
}

/// Whether a block (`None`: the first word, everything) wrote what
/// `query` reads.
fn touches<Q: ::program::Reads>(change: &Option<view::methods::Change>, query: &Q) -> bool {
    change
        .as_ref()
        .is_none_or(|change| change.touches::<huddle::Huddle, _>(query))
}

/// Why an op did not go through, or `None` when it did.
fn refused(submitted: Result<Submitted, Error>) -> Option<String> {
    match submitted {
        Ok(Submitted::Applied { .. }) => None,
        Ok(Submitted::Refused { refusal, .. }) | Err(refusal) => Some(refusal.message),
        Ok(Submitted::InDoubt { .. }) => Some(words::NO_ANSWER.into()),
    }
}
