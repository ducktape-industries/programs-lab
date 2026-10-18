//! The `huddle` program: calls between people over the node's live tracks.
//! A call is a room of this program, named by an id its caller mints; the
//! program keeps who was rung and when, who joined and who declined, and
//! writes one grant row for each person asked in, so only they publish in
//! the room, each their own camera and microphone, and hear everyone.
//! Pictures and voices never touch the chain: the hosts publish and play
//! them through the node, from the pieces the view places. The layout, in
//! reading order:
//!
//! - `lib.rs` (here): the types on the wire, all borsh, a call's clock
//!   ([`Call::rings`], [`Call::over`]), and [`describe()`]
//! - `state.rs`: the tables, among them the door's: who knocks at a call
//!   (an account with its link that was not asked in), and how long a call
//!   waits for knocks
//! - `program.rs`: [`Huddle`], the program (its name and types, which a
//!   view names it by) and its dispatch: one match over every op and one
//!   over every query
//! - `rules.rs`: what each op checks and writes
//! - `queries.rs`: one function per question
//! - `tests.rs`: the program natively over `::program::MockHost`
mod program;
mod queries;
mod rules;
pub mod state;
#[cfg(test)]
mod tests;

pub use program::Huddle;

use ::program::Env;
use ::program::live::Grant;
use borsh::{BorshDeserialize, BorshSerialize};

pub const PROGRAM: &str = "huddle";

/// How long a person is rung, in seconds, from when they were invited.
pub const RING_SECONDS: u64 = 45;

/// How long a knock waits for an answer, in seconds: a minute.
pub const KNOCK_SECONDS: u64 = 60;

/// How long a call with fewer than two in it waits for knocks, in seconds,
/// from its start, its last knock, its last Join and a Leave that leaves
/// someone in it: ten minutes. A view in the call alone sends Join again
/// well inside it, so the call waits while someone is there and is over
/// this long after they are gone without a Leave.
pub const WAIT_SECONDS: u64 = 600;

/// The most people a call holds, its caller among them.
pub const MAX_PEOPLE: usize = 16;

/// The longest call id, in bytes.
pub const MAX_CALL_ID: usize = 64;

/// The track `account`'s camera publishes into in a call.
pub fn camera_track(account: u64) -> String {
    format!("cam/{account}")
}

/// The track `account`'s microphone publishes into in a call: while it is
/// up, they are in the call (muted, it stays up and sends nothing).
pub fn microphone_track(account: u64) -> String {
    format!("mic/{account}")
}

/// The row each person asked into a call has in its room: they publish
/// their own camera and microphone, under their number (`{account}`), so no
/// one speaks as another, and hear every track.
pub fn seat() -> Grant {
    Grant {
        publish: vec!["cam/{account}".into(), "mic/{account}".into()],
        subscribe: vec!["*".into()],
    }
}

/// The chain's time in seconds, which is what a call keeps: the kernel's
/// block time is in milliseconds, and a notice's `until` is in seconds.
pub fn seconds(env: &Env) -> u64 {
    env.time / 1000
}

/// One call, as `call/<id>` keeps it. Times are the chain's, in seconds.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Call {
    /// Who started it.
    pub caller: u64,
    /// Everyone asked in, the caller aside, each with when they were last
    /// asked: their ring runs [`RING_SECONDS`] from then.
    pub invited: Vec<Invitee>,
    /// Who is in it, as the program heard: in by `Join` (the caller by
    /// `Start`), out by `Leave`. The room's live tracks say who is there
    /// now; this lags them by a block and never hears a crash.
    pub joined: Vec<u64>,
    /// Who said no.
    pub declined: Vec<u64>,
    pub started: u64,
    /// When the last person left or the caller ended it.
    pub ended: Option<u64>,
}

/// A person asked into a call, and when.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Invitee {
    pub account: u64,
    pub at: u64,
}

impl Call {
    /// Everyone with a row in the room: the caller, then each invitee.
    pub fn people(&self) -> impl Iterator<Item = u64> + '_ {
        std::iter::once(self.caller).chain(self.invited.iter().map(|invitee| invitee.account))
    }

    /// Whether `account` was asked in: the caller, or an invitee.
    pub fn asked(&self, account: u64) -> bool {
        self.people().any(|person| person == account)
    }

    /// When `account`'s ring stops; `None` for one never invited.
    pub fn ring_until(&self, account: u64) -> Option<u64> {
        self.invited
            .iter()
            .find(|invitee| invitee.account == account)
            .map(|invitee| invitee.at + RING_SECONDS)
    }

    /// Whether `account` is being rung at `now`: invited, not in the call,
    /// not declined, the call not ended, and less than [`RING_SECONDS`]
    /// since they were asked.
    pub fn rings(&self, account: u64, now: u64) -> bool {
        self.ended.is_none()
            && !self.joined.contains(&account)
            && !self.declined.contains(&account)
            && self.ring_until(account).is_some_and(|until| now < until)
    }

    /// Whether the call is over at `now`: ended, or fewer than two people
    /// in it with no one being rung, no one knocking and its wait for
    /// knocks ([`WAIT_SECONDS`], which `waits_until` ends; `None` for a
    /// call started before there were knocks) out. No one answered, or everyone else left: the
    /// program sees no track go down, so a call abandoned without a `Leave`
    /// is over by this rule, not by a write.
    pub fn over(&self, knocks: &[Knocker], waits_until: Option<u64>, now: u64) -> bool {
        self.ended.is_some()
            || (self.joined.len() < 2
                && !self
                    .invited
                    .iter()
                    .any(|invitee| self.rings(invitee.account, now))
                && !knocks.iter().any(|knocker| knocker.waits(now))
                && waits_until.is_none_or(|until| now >= until))
    }
}

/// One knock at a call's door, as `knock/<call>/<account>` keeps it: when,
/// and what was answered, until the knocker knocks again, withdraws or
/// joins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Knock {
    pub at: u64,
    pub answer: Option<Answer>,
}

/// What someone in the call answered a knock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Answer {
    Admitted,
    TurnedAway,
}

/// A knock as a list names it: whose, and the knock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Knocker {
    pub account: u64,
    pub knock: Knock,
}

/// Where a knock stands at the chain's time, as a list says it so no
/// reader needs the chain's clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Standing {
    /// Unanswered, inside its [`KNOCK_SECONDS`].
    Waiting,
    /// Unanswered past them: the knocker may knock again.
    RanOut,
    Admitted,
    TurnedAway,
}

impl Knocker {
    /// Whether the knock waits for an answer at `now`.
    pub fn waits(&self, now: u64) -> bool {
        self.knock.answer.is_none() && now < self.knock.at + KNOCK_SECONDS
    }

    pub fn standing(&self, now: u64) -> Standing {
        match self.knock.answer {
            Some(Answer::Admitted) => Standing::Admitted,
            Some(Answer::TurnedAway) => Standing::TurnedAway,
            None if self.waits(now) => Standing::Waiting,
            None => Standing::RanOut,
        }
    }
}

/// A knock as a reader gets it: whose, and where it stands at the chain's
/// time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Knocking {
    pub account: u64,
    pub standing: Standing,
}

/// A call as a list names it: its id, and the call.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Listed {
    pub id: String,
    pub call: Call,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Op {
    /// Starts call `call` (an id the caller mints, unused before) with the
    /// sender in it, ringing each of `invited` from now. Each of them and
    /// the sender gets their row in the room. With no one invited the call
    /// waits for knocks ([`WAIT_SECONDS`]): its caller shares its link.
    Start { call: String, invited: Vec<u64> },
    /// Asks `accounts` into a call the sender is in, each rung from now;
    /// one asked before rings again, one who declined is asked again, one
    /// in the call already is left as they are. A call that is over but
    /// not ended (the sender alone, no one rung) rings again.
    Invite { call: String, accounts: Vec<u64> },
    /// The sender is in the call: its caller, or one asked in. Sent again,
    /// it writes the same row, so a view sends it on each `Waiting` its
    /// tracks hear, and while alone in the call to keep it waiting for
    /// knocks ([`WAIT_SECONDS`]): from one in it already, it is refused
    /// only once the call has ended.
    Join { call: String },
    /// The sender is out of the call, their row taken back; the last one
    /// out ends it, and every row goes. A sender who was knocking, not in
    /// it, withdraws their knock; let in already, they decline.
    Leave { call: String },
    /// The sender, asked in, says no: their ring stops, their row goes.
    Decline { call: String },
    /// The caller ends the call for everyone: every row goes.
    End { call: String },
    /// The sender, not asked into a call that is not over, asks to join
    /// it: their knock waits [`KNOCK_SECONDS`] for someone in the call to
    /// answer. Knocked again, it waits anew.
    Knock { call: String },
    /// Someone in the call lets `account`, whose knock waits, in: they are
    /// asked in as an invite asks, their row written, but not rung (they
    /// asked).
    Admit { call: String, account: u64 },
    /// Someone in the call turns `account`'s waiting knock away.
    TurnAway { call: String, account: u64 },
}

/// A read. Each variant names the reply that answers it, so a view asks it
/// alone (`host.query(huddle::ask::Mine { .. })`) and gets that reply, and
/// the tables it reads, so a view asks again only when a block wrote one.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, ::program::Ask)]
#[ask(Huddle)]
pub enum Query {
    /// The call `id`, over or not.
    #[ask(Reply::Call(Option<Call>))]
    #[reads(state::CALLS)]
    Call { id: String },
    /// The calls `account` is in, or asked into and has not declined,
    /// that are not over at the chain's time; newest first.
    #[ask(Reply::Calls(Vec<Listed>))]
    #[reads(state::CALLS, state::MINE, state::KNOCKS, state::WAITS)]
    Mine { account: u64 },
    /// The calls ringing `account` at the chain's time ([`Call::rings`]);
    /// newest first. A ring that ran out is not here, so a reader catching
    /// up after a while away never rings a call that is over. Nor is a call
    /// that admitted `account`'s knock: they asked, so nothing rings them.
    #[ask(Reply::Calls(Vec<Listed>))]
    #[reads(state::CALLS, state::MINE, state::KNOCKS)]
    Ringing { account: u64 },
    /// Everyone who knocked at call `call` and has not knocked again,
    /// withdrawn or joined since, each where their knock stands at the
    /// chain's time; `None` for no such call.
    #[ask(Reply::Knocks(Option<Vec<Knocking>>))]
    #[reads(state::CALLS, state::KNOCKS)]
    Knocks { call: String },
    /// The knocks waiting at the doors of the calls `account` is in, each
    /// call with its knockers; newest call first.
    #[ask(Reply::Doors(Vec<Door>))]
    #[reads(state::CALLS, state::MINE, state::KNOCKS)]
    Doors { account: u64 },
    /// The calls that let `account` in at their door and that they have
    /// not joined, declined, withdrawn from or seen go over since, inside
    /// [`WAIT_SECONDS`] of being let in; newest first. Not in `Ringing`:
    /// they asked, so nothing rings them.
    #[ask(Reply::Calls(Vec<Listed>))]
    #[reads(state::CALLS, state::MINE, state::KNOCKS, state::WAITS)]
    LetIn { account: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Reply {
    Call(Option<Call>),
    Calls(Vec<Listed>),
    Knocks(Option<Vec<Knocking>>),
    Doors(Vec<Door>),
}

/// A call's door as someone in it reads it: the call, and the knocks
/// waiting at it (each inside its [`KNOCK_SECONDS`]).
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Door {
    pub call: String,
    pub waiting: Vec<Knocker>,
}

/// An op as a person reads it: a title and its fields. The source of the
/// `ducktape.describe` module this program ships (`make wasm-describes`).
pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let accounts =
        |accounts: &[u64]| Value::List(accounts.iter().copied().map(Value::Account).collect());
    let (title, fields) = match op {
        Op::Start { call, invited } => (
            "Call people".to_owned(),
            vec![
                field("call", Value::text(call)),
                field("calling", accounts(invited)),
            ],
        ),
        Op::Invite {
            call,
            accounts: asked,
        } => (
            "Ask people into a call".to_owned(),
            vec![
                field("call", Value::text(call)),
                field("asking", accounts(asked)),
            ],
        ),
        Op::Join { call } => (
            "Join a call".to_owned(),
            vec![field("call", Value::text(call))],
        ),
        Op::Leave { call } => (
            "Leave a call".to_owned(),
            vec![field("call", Value::text(call))],
        ),
        Op::Decline { call } => (
            "Decline a call".to_owned(),
            vec![field("call", Value::text(call))],
        ),
        Op::End { call } => (
            "End a call for everyone".to_owned(),
            vec![field("call", Value::text(call))],
        ),
        Op::Knock { call } => (
            "Ask to join a call".to_owned(),
            vec![field("call", Value::text(call))],
        ),
        Op::Admit { call, account } => (
            "Let someone into a call".to_owned(),
            vec![
                field("call", Value::text(call)),
                field("letting in", Value::Account(*account)),
            ],
        ),
        Op::TurnAway { call, account } => (
            "Turn someone away from a call".to_owned(),
            vec![
                field("call", Value::text(call)),
                field("turning away", Value::Account(*account)),
            ],
        ),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

/// Old op bytes are described with the current code (`describe`): the op
/// enum only grows at its end. Append a new variant here; never reorder.
#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        [
            "Start", "Invite", "Join", "Leave", "Decline", "End", "Knock", "Admit", "TurnAway"
        ]
    );
}
