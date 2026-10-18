//! The ops once as meant and once as an attack: each refusal leaves the
//! store exactly as it was. The harness is the [`Huddle`] program over a
//! [`MockHost`] with a clock the test moves; the accounts sign the env of
//! their own ops.
use ::program::live::Grant;
use ::program::{Env, Error, MockHost, Program, QueryCtx, code};

use crate::{
    Call, Door, Huddle, KNOCK_SECONDS, Knock, Knocker, Knocking, Listed, MAX_PEOPLE, Op, PROGRAM,
    Query, RING_SECONDS, Reply, Standing, WAIT_SECONDS, seat,
};

const ADA: u64 = 1;
const BO: u64 = 2;
const CY: u64 = 3;
const DEE: u64 = 4;

const CALL: &str = "call-1";

#[derive(Default)]
struct Line {
    store: MockHost,
    /// The chain's time, in seconds.
    now: u64,
}

impl Line {
    /// The env at the chain's time: the kernel's is in milliseconds.
    fn env(&self) -> Env {
        Env {
            time: self.now * 1000,
            ..MockHost::env(PROGRAM)
        }
    }

    fn reads(&self) -> QueryCtx {
        self.store.query(self.env())
    }

    /// Sends `op` as `who`. A refusal has left the store as it found it,
    /// or this panics.
    #[track_caller]
    fn send(&mut self, who: u64, op: Op) -> Result<(), Error> {
        let signed = self.env().signed(who.to_be_bytes(), Some(who));
        let ctx = self.store.exec(signed);
        self.store.attempt(|| Huddle::execute(&ctx, op))
    }

    #[track_caller]
    fn ok(&mut self, who: u64, op: Op) {
        if let Err(refusal) = self.send(who, op) {
            panic!("refused: {refusal}");
        }
    }

    #[track_caller]
    fn refused(&mut self, who: u64, op: Op) -> &'static str {
        let refusal = self.send(who, op).expect_err("refused");
        [
            code::UNAUTHORIZED,
            code::WRONG_STATE,
            code::NOT_FOUND,
            code::ALREADY_EXISTS,
            code::INVALID_INPUT,
            code::CAPACITY,
        ]
        .into_iter()
        .find(|known| refusal.code == *known)
        .unwrap_or_else(|| panic!("refused as {}", refusal.code))
    }

    fn start(&mut self, caller: u64, invited: &[u64]) {
        self.ok(
            caller,
            Op::Start {
                call: CALL.into(),
                invited: invited.to_vec(),
            },
        );
    }

    fn call(&self) -> Call {
        let Ok(Reply::Call(Some(call))) =
            Huddle::query(&self.reads(), Query::Call { id: CALL.into() })
        else {
            panic!("a call answers its call");
        };
        call
    }

    /// Whether the call is over at the chain's time, as the program reads
    /// it: not among its caller's calls.
    fn over(&self) -> bool {
        let call = self.call();
        !self.mine(call.caller).contains(&CALL.to_owned())
    }

    fn knocks(&self) -> Vec<Knocking> {
        let Ok(Reply::Knocks(Some(knocks))) =
            Huddle::query(&self.reads(), Query::Knocks { call: CALL.into() })
        else {
            panic!("a call answers its knocks");
        };
        knocks
    }

    fn doors(&self, account: u64) -> Vec<Door> {
        let Ok(Reply::Doors(doors)) = Huddle::query(&self.reads(), Query::Doors { account }) else {
            panic!("doors answer doors");
        };
        doors
    }

    fn knock(&mut self, who: u64) -> Result<(), Error> {
        self.send(who, Op::Knock { call: CALL.into() })
    }

    fn admit(&mut self, who: u64, account: u64) -> Result<(), Error> {
        self.send(
            who,
            Op::Admit {
                call: CALL.into(),
                account,
            },
        )
    }

    fn turn_away(&mut self, who: u64, account: u64) -> Result<(), Error> {
        self.send(
            who,
            Op::TurnAway {
                call: CALL.into(),
                account,
            },
        )
    }

    fn calls(&self, query: Query) -> Vec<String> {
        let Ok(Reply::Calls(calls)) = Huddle::query(&self.reads(), query) else {
            panic!("a list answers calls");
        };
        calls.into_iter().map(|Listed { id, .. }| id).collect()
    }

    fn ringing(&self, account: u64) -> Vec<String> {
        self.calls(Query::Ringing { account })
    }

    fn mine(&self, account: u64) -> Vec<String> {
        self.calls(Query::Mine { account })
    }

    fn let_in(&self, account: u64) -> Vec<String> {
        self.calls(Query::LetIn { account })
    }

    /// `who`'s row in the room of `call`; `None` for `*`.
    fn row(&self, call: &str, who: Option<u64>) -> Option<Grant> {
        let ctx = self.store.exec(self.env());
        ctx.record(::program::live::key(call, who))
            .expect("a row decodes")
    }

    fn state(&self) -> std::collections::BTreeMap<Vec<u8>, Vec<u8>> {
        self.store.borrow().state.clone()
    }
}

#[test]
fn start_seats_the_caller_and_each_invitee_and_opens_the_room_to_no_one_else() {
    let mut line = Line {
        now: 100,
        ..Line::default()
    };
    line.start(ADA, &[BO, CY, BO, ADA]);
    let call = line.call();
    assert_eq!((call.caller, call.started, call.ended), (ADA, 100, None));
    assert_eq!(call.joined, [ADA]);
    let invited: Vec<_> = call.invited.iter().map(|i| (i.account, i.at)).collect();
    assert_eq!(
        invited,
        [(BO, 100), (CY, 100)],
        "once each, the caller aside"
    );
    for who in [ADA, BO, CY] {
        assert_eq!(line.row(CALL, Some(who)), Some(seat()), "{who}'s row");
    }
    assert_eq!(
        line.row(CALL, None),
        None,
        "no `*` row: no one else gets in"
    );
    assert_eq!(line.row(CALL, Some(DEE)), None);
    // each publishes their own camera and microphone, and hears everyone
    let row = seat();
    assert!(row.publishes("mic/2", BO) && row.publishes("cam/2", BO));
    assert!(!row.publishes("mic/1", BO));
    assert!(row.subscribes("cam/1", BO));
    assert_eq!(line.ringing(BO), [CALL]);
    assert_eq!(
        line.ringing(ADA),
        Vec::<String>::new(),
        "the caller is not rung"
    );
    assert_eq!(line.mine(ADA), [CALL]);
    assert_eq!(line.mine(DEE), Vec::<String>::new());
}

#[test]
fn a_start_that_cannot_be_a_call_is_refused() {
    let mut line = Line::default();
    let start = |call: &str, invited: Vec<u64>| Op::Start {
        call: call.into(),
        invited,
    };
    let empty = line.state();
    let long = "x".repeat(65);
    for id in ["", "a/b", "call 1", "*", long.as_str()] {
        assert_eq!(
            line.refused(ADA, start(id, vec![BO])),
            code::INVALID_INPUT,
            "{id:?}"
        );
    }
    let crowd: Vec<u64> = (10..10 + MAX_PEOPLE as u64).collect();
    assert_eq!(line.refused(ADA, start(CALL, crowd)), code::CAPACITY);
    assert_eq!(line.state(), empty);
    line.start(ADA, &[BO]);
    assert_eq!(
        line.refused(CY, start(CALL, vec![DEE])),
        code::ALREADY_EXISTS
    );
    assert_eq!(line.call().caller, ADA, "no one takes over a call");
}

#[test]
fn join_is_idempotent_and_only_for_one_asked_in() {
    let mut line = Line::default();
    line.start(ADA, &[BO]);
    let join = || Op::Join { call: CALL.into() };
    line.now = 10;
    line.ok(BO, join());
    assert_eq!(line.call().joined, [ADA, BO]);
    let joined = line.state();
    // the view's re-ask on `Waiting`: the same row, nothing new
    line.ok(BO, join());
    assert_eq!(line.state(), joined);
    assert_eq!(line.refused(DEE, join()), code::UNAUTHORIZED);
    assert_eq!(line.row(CALL, Some(DEE)), None);
    assert_eq!(
        line.refused(
            BO,
            Op::Join {
                call: "nowhere".into()
            }
        ),
        code::NOT_FOUND
    );
    // the caller ends it: no one joins an ended call
    line.ok(ADA, Op::End { call: CALL.into() });
    assert_eq!(line.refused(BO, join()), code::WRONG_STATE);
    assert_eq!(line.refused(ADA, join()), code::WRONG_STATE);
}

#[test]
fn the_last_one_out_ends_the_call_and_every_row_goes() {
    let mut line = Line::default();
    line.start(ADA, &[BO, CY]);
    line.ok(BO, Op::Join { call: CALL.into() });
    line.now = 60;
    line.ok(ADA, Op::Leave { call: CALL.into() });
    assert_eq!(line.call().ended, None, "Bo is still in it");
    assert_eq!(line.row(CALL, Some(ADA)), None, "the leaver's row went");
    assert_eq!(line.row(CALL, Some(BO)), Some(seat()));
    assert_eq!(line.mine(ADA), Vec::<String>::new());
    assert_eq!(
        line.refused(ADA, Op::Leave { call: CALL.into() }),
        code::WRONG_STATE,
        "out already"
    );
    line.now = 75;
    line.ok(BO, Op::Leave { call: CALL.into() });
    assert_eq!(line.call().ended, Some(75));
    for who in [ADA, BO, CY] {
        assert_eq!(line.row(CALL, Some(who)), None, "{who}'s row");
        assert_eq!(line.mine(who), Vec::<String>::new());
    }
    assert_eq!(
        line.refused(BO, Op::Leave { call: CALL.into() }),
        code::WRONG_STATE
    );
}

#[test]
fn end_is_the_callers_and_takes_every_row() {
    let mut line = Line::default();
    line.start(ADA, &[BO, CY]);
    line.ok(BO, Op::Join { call: CALL.into() });
    assert_eq!(
        line.refused(BO, Op::End { call: CALL.into() }),
        code::UNAUTHORIZED
    );
    line.now = 20;
    line.ok(ADA, Op::End { call: CALL.into() });
    assert_eq!(line.call().ended, Some(20));
    for who in [ADA, BO, CY] {
        assert_eq!(line.row(CALL, Some(who)), None, "{who}'s row");
    }
    assert_eq!(line.ringing(CY), Vec::<String>::new());
    assert_eq!(
        line.refused(ADA, Op::End { call: CALL.into() }),
        code::WRONG_STATE
    );
}

#[test]
fn decline_takes_back_the_decliners_row_alone() {
    let mut line = Line::default();
    line.start(ADA, &[BO, CY]);
    line.ok(CY, Op::Decline { call: CALL.into() });
    assert_eq!(line.call().declined, [CY]);
    assert_eq!(line.row(CALL, Some(CY)), None);
    assert_eq!(line.row(CALL, Some(BO)), Some(seat()));
    assert_eq!(line.ringing(CY), Vec::<String>::new());
    assert_eq!(line.mine(CY), Vec::<String>::new());
    assert_eq!(line.ringing(BO), [CALL]);
    // again: nothing new
    let declined = line.state();
    line.ok(CY, Op::Decline { call: CALL.into() });
    assert_eq!(line.state(), declined);
    // the caller and a stranger were not invited; one in the call leaves
    for who in [ADA, DEE] {
        assert_eq!(
            line.refused(who, Op::Decline { call: CALL.into() }),
            code::UNAUTHORIZED
        );
    }
    line.ok(BO, Op::Join { call: CALL.into() });
    assert_eq!(
        line.refused(BO, Op::Decline { call: CALL.into() }),
        code::WRONG_STATE
    );
    // they change their mind while it goes on
    line.ok(CY, Op::Join { call: CALL.into() });
    assert_eq!(line.call().declined, Vec::<u64>::new());
    assert_eq!(line.row(CALL, Some(CY)), Some(seat()));
}

#[test]
fn ringing_is_the_invited_not_in_not_declined_inside_their_ring_by_the_chains_clock() {
    let mut line = Line {
        now: 1_000,
        ..Line::default()
    };
    line.start(ADA, &[BO, CY, DEE]);
    line.ok(BO, Op::Join { call: CALL.into() });
    line.ok(CY, Op::Decline { call: CALL.into() });
    assert_eq!(line.ringing(BO), Vec::<String>::new(), "joined");
    assert_eq!(line.ringing(CY), Vec::<String>::new(), "declined");
    assert_eq!(line.ringing(DEE), [CALL]);
    // the last second of the ring, then past it: the env's time is
    // milliseconds, the ring's seconds
    line.now = 1_000 + RING_SECONDS - 1;
    assert_eq!(line.ringing(DEE), [CALL]);
    line.now = 1_000 + RING_SECONDS;
    assert_eq!(line.ringing(DEE), Vec::<String>::new(), "rung out");
    assert_eq!(line.call().ring_until(DEE), Some(1_000 + RING_SECONDS));
    // the call goes on: Ada and Bo are in it, so it is no one's to ring
    // but still Dee's to join
    assert_eq!(line.mine(DEE), [CALL]);
    line.ok(DEE, Op::Join { call: CALL.into() });
    // ended: no one rings
    let mut ended = Line {
        now: 1_000,
        ..Line::default()
    };
    ended.start(ADA, &[BO]);
    ended.ok(ADA, Op::End { call: CALL.into() });
    assert_eq!(ended.ringing(BO), Vec::<String>::new());
}

#[test]
fn a_later_invitee_rings_on_their_own_clock() {
    let mut line = Line::default();
    line.start(ADA, &[BO]);
    line.now = 30;
    line.ok(
        ADA,
        Op::Invite {
            call: CALL.into(),
            accounts: vec![CY, ADA],
        },
    );
    assert_eq!(line.row(CALL, Some(CY)), Some(seat()));
    assert_eq!(line.call().ring_until(CY), Some(30 + RING_SECONDS));
    line.now = RING_SECONDS;
    assert_eq!(line.ringing(BO), Vec::<String>::new(), "Bo's ring ran out");
    assert_eq!(line.ringing(CY), [CALL], "Cy's runs on");
    // no one answered Bo, and Cy still rings: the call is not over
    line.ok(
        ADA,
        Op::Invite {
            call: CALL.into(),
            accounts: vec![BO],
        },
    );
    assert_eq!(line.ringing(BO), [CALL], "asked again, rung again");
    // a stranger and one rung but not in the call ask no one in
    for who in [DEE, CY] {
        assert_eq!(
            line.refused(
                who,
                Op::Invite {
                    call: CALL.into(),
                    accounts: vec![DEE],
                }
            ),
            code::UNAUTHORIZED
        );
    }
    // one who declined is asked again
    line.ok(CY, Op::Decline { call: CALL.into() });
    line.ok(
        ADA,
        Op::Invite {
            call: CALL.into(),
            accounts: vec![CY],
        },
    );
    assert_eq!(line.ringing(CY), [CALL]);
    assert_eq!(line.row(CALL, Some(CY)), Some(seat()));
    // past the most a call holds
    let crowd: Vec<u64> = (10..10 + MAX_PEOPLE as u64).collect();
    assert_eq!(
        line.refused(
            ADA,
            Op::Invite {
                call: CALL.into(),
                accounts: crowd,
            }
        ),
        code::CAPACITY
    );
}

/// A call no one answered waits for knocks past its rings, then is over:
/// its caller alone, no one rung or knocking, its wait out. It still
/// closes.
#[test]
fn a_call_no_one_answered_is_over_once_its_wait_runs_out_and_still_closes() {
    let mut line = Line::default();
    line.start(ADA, &[BO]);
    line.now = RING_SECONDS;
    assert!(!line.over(), "rung out, but waiting for knocks");
    line.now = WAIT_SECONDS - 1;
    assert!(!line.over());
    line.now = WAIT_SECONDS;
    assert!(line.over());
    assert_eq!(line.mine(ADA), Vec::<String>::new());
    assert_eq!(
        line.refused(BO, Op::Join { call: CALL.into() }),
        code::WRONG_STATE
    );
    // the caller's view leaves: the rows go
    line.ok(ADA, Op::Leave { call: CALL.into() });
    assert_eq!(line.call().ended, Some(WAIT_SECONDS));
    assert_eq!(line.row(CALL, Some(BO)), None);
}

/// A call from before there were knocks (no wait row) is over as it was:
/// once its rings run out.
#[test]
fn an_old_shape_call_is_over_as_before() {
    let mut line = Line::default();
    line.start(ADA, &[BO]);
    // as garage holds it: the call row alone
    let ctx = line.store.exec(line.env());
    ctx.delete(crate::state::WAITS.key(&CALL.to_owned()));
    assert!(!line.over());
    line.now = RING_SECONDS;
    assert!(line.over(), "rung out, no wait");
    assert_eq!(
        line.refused(DEE, Op::Knock { call: CALL.into() }),
        code::WRONG_STATE
    );
    // its caller's Invite still rings again
    line.ok(
        ADA,
        Op::Invite {
            call: CALL.into(),
            accounts: vec![CY],
        },
    );
    assert!(!line.over());
    line.now = RING_SECONDS * 3;
    assert!(line.over(), "Cy's ring ran out too");
    // its caller, still in it, keeps it by Join: it waits as a call of now
    line.ok(ADA, Op::Join { call: CALL.into() });
    assert!(!line.over());
}

/// The caller left alone in the room by a decline asks someone in again,
/// and it rings; once the call has ended, it does not.
#[test]
fn the_caller_alone_after_a_decline_can_invite() {
    let mut line = Line::default();
    line.start(ADA, &[BO]);
    line.ok(BO, Op::Decline { call: CALL.into() });
    line.now = WAIT_SECONDS;
    assert!(line.over());
    let invite = |accounts: Vec<u64>| Op::Invite {
        call: CALL.into(),
        accounts,
    };
    line.ok(ADA, invite(vec![CY]));
    assert!(!line.over());
    assert_eq!(line.ringing(CY), [CALL]);
    assert_eq!(line.row(CALL, Some(CY)), Some(seat()));
    line.ok(ADA, Op::End { call: CALL.into() });
    assert_eq!(line.refused(ADA, invite(vec![DEE])), code::WRONG_STATE);
}

// ---------- the door ----------

/// A call starts with no one rung: its caller alone, waiting for knocks
/// ten minutes from its start, and ten minutes from each knock after.
#[test]
fn a_call_starts_alone_and_waits_for_knocks() {
    let mut line = Line {
        now: 100,
        ..Line::default()
    };
    line.start(ADA, &[]);
    let call = line.call();
    assert_eq!(
        (call.joined.as_slice(), call.invited.len()),
        (&[ADA][..], 0)
    );
    assert_eq!(line.row(CALL, Some(ADA)), Some(seat()));
    assert_eq!(line.row(CALL, None), None, "no `*` row");
    assert_eq!(line.mine(ADA), [CALL]);
    line.now = 100 + WAIT_SECONDS - 1;
    assert!(!line.over());
    // a knock at the last second keeps it open ten minutes more
    line.knock(BO).expect("a knock");
    line.now = 100 + WAIT_SECONDS;
    assert!(!line.over(), "someone is knocking");
    line.now = 100 + WAIT_SECONDS + KNOCK_SECONDS;
    assert!(!line.over(), "the knock ran out, the wait from it runs");
    line.now = 100 + WAIT_SECONDS - 1 + WAIT_SECONDS;
    assert!(line.over());
    assert_eq!(
        line.refused(CY, Op::Knock { call: CALL.into() }),
        code::WRONG_STATE,
        "over: no more knocks"
    );
    // abandoned, it still closes by its caller's Leave
    line.ok(ADA, Op::Leave { call: CALL.into() });
    assert!(line.call().ended.is_some());
    assert_eq!(
        line.knocks(),
        Vec::<Knocking>::new(),
        "the door's rows went"
    );
}

/// One alone in a call keeps it waiting for knocks by Join, sent again
/// inside the wait; over with no write since (their view asleep), their
/// Join keeps it again. Once it has ended, it does not.
#[test]
fn one_alone_in_a_call_keeps_it_waiting_by_join() {
    let mut line = Line::default();
    line.start(ADA, &[]);
    let join = || Op::Join { call: CALL.into() };
    line.now = WAIT_SECONDS / 2;
    line.ok(ADA, join());
    line.now = WAIT_SECONDS;
    assert!(!line.over(), "the wait runs from the Join");
    line.knock(BO).expect("the door is open");
    line.now = WAIT_SECONDS * 3;
    assert!(line.over(), "nothing said for ten minutes");
    assert_eq!(
        line.refused(CY, Op::Knock { call: CALL.into() }),
        code::WRONG_STATE
    );
    assert_eq!(line.refused(BO, join()), code::UNAUTHORIZED, "never asked");
    line.ok(ADA, join());
    assert!(!line.over(), "still there: kept");
    line.knock(CY).expect("the door is open again");
    line.ok(ADA, Op::End { call: CALL.into() });
    assert_eq!(line.refused(ADA, join()), code::WRONG_STATE);
}

/// Whoever a Leave leaves in the call waits for knocks from then, however
/// long the call ran.
#[test]
fn the_one_a_leave_leaves_waits_for_knocks() {
    let mut line = Line::default();
    line.start(ADA, &[BO]);
    line.ok(BO, Op::Join { call: CALL.into() });
    line.now = WAIT_SECONDS * 3;
    assert!(!line.over(), "two in it");
    line.ok(BO, Op::Leave { call: CALL.into() });
    line.now = WAIT_SECONDS * 4 - 1;
    assert!(!line.over());
    line.knock(CY).expect("the door is open");
}

/// A call that let a knocker in is theirs to open until they join,
/// withdraw or decline, or the wait from it runs out; no one else's.
#[test]
fn let_in_lists_a_call_that_let_its_knocker_in() {
    let mut line = Line::default();
    line.start(ADA, &[]);
    line.knock(CY).expect("a knock");
    assert_eq!(line.let_in(CY), Vec::<String>::new(), "not let in yet");
    line.admit(ADA, CY).expect("admitted");
    assert_eq!(line.let_in(CY), [CALL]);
    assert_eq!(line.let_in(ADA), Vec::<String>::new());
    line.ok(CY, Op::Join { call: CALL.into() });
    assert_eq!(line.let_in(CY), Vec::<String>::new(), "in");
    line.knock(DEE).expect("a knock");
    line.admit(ADA, DEE).expect("admitted");
    line.ok(DEE, Op::Decline { call: CALL.into() });
    assert_eq!(line.let_in(DEE), Vec::<String>::new(), "declined");
    line.now = 10;
    line.knock(BO).expect("a knock");
    line.admit(CY, BO).expect("admitted");
    line.now = 10 + WAIT_SECONDS - 1;
    assert_eq!(line.let_in(BO), [CALL]);
    line.now = 10 + WAIT_SECONDS;
    assert_eq!(line.let_in(BO), Vec::<String>::new(), "the wait ran out");
}

/// A knock waits a minute for an answer, then runs out; knocked again, it
/// waits anew. One who was asked in does not knock, nor does anyone at a
/// call that has ended or never was.
#[test]
fn a_knock_waits_a_minute_and_can_be_knocked_again() {
    let mut line = Line {
        now: 1_000,
        ..Line::default()
    };
    line.start(ADA, &[BO]);
    let empty = line.state();
    assert_eq!(
        line.refused(BO, Op::Knock { call: CALL.into() }),
        code::WRONG_STATE
    );
    assert_eq!(
        line.refused(ADA, Op::Knock { call: CALL.into() }),
        code::WRONG_STATE
    );
    assert_eq!(
        line.refused(
            CY,
            Op::Knock {
                call: "nowhere".into()
            }
        ),
        code::NOT_FOUND
    );
    assert_eq!(line.state(), empty);
    line.knock(CY).expect("a knock");
    let waiting = |account| Knocking {
        account,
        standing: Standing::Waiting,
    };
    assert_eq!(line.knocks(), [waiting(CY)]);
    assert_eq!(
        line.doors(ADA),
        [Door {
            call: CALL.into(),
            waiting: vec![Knocker {
                account: CY,
                knock: Knock {
                    at: 1_000,
                    answer: None,
                },
            }],
        }]
    );
    assert_eq!(line.doors(BO), Vec::<Door>::new(), "rung, not in it");
    assert_eq!(line.row(CALL, Some(CY)), None, "no row for knocking");
    assert_eq!(line.mine(CY), Vec::<String>::new());
    line.now = 1_000 + KNOCK_SECONDS - 1;
    assert_eq!(line.knocks(), [waiting(CY)]);
    line.now = 1_000 + KNOCK_SECONDS;
    assert_eq!(
        line.knocks(),
        [Knocking {
            account: CY,
            standing: Standing::RanOut,
        }]
    );
    assert_eq!(line.doors(ADA), Vec::<Door>::new());
    // again, at once: no cooldown
    line.knock(CY).expect("knocked again");
    assert_eq!(line.knocks(), [waiting(CY)]);
    line.knock(CY).expect("and again");
    assert_eq!(line.knocks().len(), 1, "one knock per knocker");
    line.ok(ADA, Op::End { call: CALL.into() });
    assert_eq!(
        line.refused(CY, Op::Knock { call: CALL.into() }),
        code::WRONG_STATE
    );
    assert_eq!(
        line.knocks(),
        Vec::<Knocking>::new(),
        "the end took the door's rows"
    );
}

/// A knocker withdraws by Leave: their knock goes and nothing else moves.
#[test]
fn a_knocker_withdraws_by_leave() {
    let mut line = Line::default();
    line.start(ADA, &[]);
    assert_eq!(
        line.refused(CY, Op::Leave { call: CALL.into() }),
        code::WRONG_STATE,
        "nothing to withdraw"
    );
    line.knock(CY).expect("a knock");
    let knocked = line.state();
    line.ok(CY, Op::Leave { call: CALL.into() });
    assert_eq!(line.knocks(), Vec::<Knocking>::new());
    assert_eq!(line.call().ended, None, "the caller is still in it");
    assert_ne!(line.state(), knocked);
    assert_eq!(line.row(CALL, Some(ADA)), Some(seat()));
}

/// Admit asks the knocker in as an invite does: their row, the call among
/// theirs, their Join at once; but no ring. Only someone in the call
/// admits, only a knock that waits is admitted, and the call's room is
/// not over-filled.
#[test]
fn admit_lets_the_knocker_join_without_a_ring() {
    let mut line = Line::default();
    line.start(ADA, &[BO]);
    line.knock(CY).expect("a knock");
    line.knock(DEE).expect("another");
    for who in [BO, DEE] {
        assert_eq!(
            line.admit(who, CY).expect_err("refused").code,
            code::UNAUTHORIZED,
            "{who} is not in the call"
        );
    }
    assert_eq!(
        line.admit(ADA, 99).expect_err("refused").code,
        code::NOT_FOUND
    );
    line.admit(ADA, CY).expect("admitted");
    assert_eq!(
        line.knocks(),
        [
            Knocking {
                account: CY,
                standing: Standing::Admitted,
            },
            Knocking {
                account: DEE,
                standing: Standing::Waiting,
            },
        ]
    );
    assert_eq!(line.row(CALL, Some(CY)), Some(seat()));
    assert_eq!(line.mine(CY), [CALL]);
    assert!(line.call().asked(CY));
    assert_eq!(
        line.ringing(CY),
        Vec::<String>::new(),
        "they asked: no ring"
    );
    assert_eq!(line.ringing(BO), [CALL], "Bo's ring goes on");
    assert_eq!(
        line.admit(ADA, CY).expect_err("answered already").code,
        code::WRONG_STATE
    );
    line.ok(CY, Op::Join { call: CALL.into() });
    assert_eq!(line.call().joined, [ADA, CY]);
    assert_eq!(line.knocks().len(), 1, "in: the knock has done its work");
    // once in, Cy admits too
    line.admit(CY, DEE).expect("admitted by one let in");
    assert_eq!(line.row(CALL, Some(DEE)), Some(seat()));
}

/// A knock that ran out asks no more: no one lets its knocker in or turns
/// them away until they knock again.
#[test]
fn a_knock_that_ran_out_is_not_answered() {
    let mut line = Line::default();
    line.start(ADA, &[]);
    line.knock(CY).expect("a knock");
    line.now = KNOCK_SECONDS;
    let ran_out = line.state();
    assert_eq!(
        line.admit(ADA, CY).expect_err("ran out").code,
        code::WRONG_STATE
    );
    assert_eq!(
        line.turn_away(ADA, CY).expect_err("ran out").code,
        code::WRONG_STATE
    );
    assert_eq!(line.state(), ran_out);
    line.knock(CY).expect("knocked again");
    line.admit(ADA, CY).expect("admitted");
}

/// One let in who withdraws before they join says no, as a Decline does:
/// their row goes, and nothing rings them.
#[test]
fn a_knocker_let_in_who_withdraws_declines() {
    let mut line = Line::default();
    line.start(ADA, &[]);
    line.knock(CY).expect("a knock");
    line.admit(ADA, CY).expect("admitted");
    line.ok(CY, Op::Leave { call: CALL.into() });
    assert_eq!(line.ringing(CY), Vec::<String>::new(), "no ring");
    assert_eq!(line.row(CALL, Some(CY)), None);
    assert_eq!(line.mine(CY), Vec::<String>::new());
    assert_eq!(line.call().declined, [CY]);
    assert_eq!(line.knocks(), Vec::<Knocking>::new());
    // asked in still: their link joins
    line.ok(CY, Op::Join { call: CALL.into() });
    assert_eq!(line.call().joined, [ADA, CY]);
}

/// Turned away, the knock stays answered until the knocker knocks again
/// or withdraws; the call is unmoved.
#[test]
fn turn_away_answers_the_knock_and_nothing_else() {
    let mut line = Line::default();
    line.start(ADA, &[]);
    line.knock(CY).expect("a knock");
    assert_eq!(
        line.turn_away(BO, CY).expect_err("refused").code,
        code::UNAUTHORIZED
    );
    assert_eq!(
        line.turn_away(ADA, DEE).expect_err("refused").code,
        code::NOT_FOUND
    );
    let call = line.call();
    line.turn_away(ADA, CY).expect("turned away");
    assert_eq!(
        line.knocks(),
        [Knocking {
            account: CY,
            standing: Standing::TurnedAway,
        }]
    );
    assert_eq!(line.doors(ADA), Vec::<Door>::new());
    assert_eq!(line.call(), call);
    assert_eq!(line.row(CALL, Some(CY)), None);
    assert_eq!(
        line.turn_away(ADA, CY).expect_err("answered already").code,
        code::WRONG_STATE
    );
    assert_eq!(
        line.admit(ADA, CY).expect_err("answered already").code,
        code::WRONG_STATE
    );
    // they knock again, as often as they like
    line.knock(CY).expect("knocked again");
    assert_eq!(line.knocks()[0].standing, Standing::Waiting);
    line.ok(CY, Op::Leave { call: CALL.into() });
    assert_eq!(line.knocks(), Vec::<Knocking>::new());
}

/// The door holds as many as the call has room for: past MAX_PEOPLE, the
/// people in or asked in with those waiting at the door, a knock is
/// refused; one that ran out holds no place.
#[test]
fn the_door_holds_as_many_as_the_call_has_room_for() {
    let mut line = Line::default();
    line.start(ADA, &[BO]);
    for who in 10..10 + (MAX_PEOPLE as u64 - 2) {
        line.knock(who).expect("a knock");
    }
    assert_eq!(line.knock(99).expect_err("full").code, code::CAPACITY);
    line.knock(10).expect("one knocking already knocks again");
    line.now = KNOCK_SECONDS;
    line.knock(99).expect("the others ran out");
}

#[test]
fn a_key_that_holds_no_account_calls_no_one() {
    let line = Line::default();
    let unheld = line.env().signed([9u8; 32], None);
    let ctx = line.store.exec(unheld);
    let refusal = line.store.refused(|| {
        Huddle::execute(
            &ctx,
            Op::Start {
                call: CALL.into(),
                invited: vec![BO],
            },
        )
    });
    assert_eq!(refusal.code, code::UNAUTHORIZED);
}

#[test]
fn an_op_reads_as_a_person_would_say_it() {
    use ::describe::{Value, field};
    let said = crate::describe(&Op::Start {
        call: CALL.into(),
        invited: vec![BO, CY],
    });
    assert_eq!(said.title, "Call people");
    assert_eq!(
        said.fields,
        [
            field("call", Value::text(CALL)),
            field(
                "calling",
                Value::List(vec![Value::Account(BO), Value::Account(CY)])
            ),
        ]
    );
    let titles = [
        Op::Invite {
            call: CALL.into(),
            accounts: vec![DEE],
        },
        Op::Join { call: CALL.into() },
        Op::Leave { call: CALL.into() },
        Op::Decline { call: CALL.into() },
        Op::End { call: CALL.into() },
        Op::Knock { call: CALL.into() },
        Op::Admit {
            call: CALL.into(),
            account: DEE,
        },
        Op::TurnAway {
            call: CALL.into(),
            account: DEE,
        },
    ]
    .map(|op| crate::describe(&op).title);
    assert_eq!(
        titles,
        [
            "Ask people into a call",
            "Join a call",
            "Leave a call",
            "Decline a call",
            "End a call for everyone",
            "Ask to join a call",
            "Let someone into a call",
            "Turn someone away from a call",
        ]
    );
    let admit = crate::describe(&Op::Admit {
        call: CALL.into(),
        account: DEE,
    });
    assert_eq!(
        admit.fields,
        [
            field("call", Value::text(CALL)),
            field("letting in", Value::Account(DEE)),
        ]
    );
}
