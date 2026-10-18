//! Huddle's notifier against a fake host whose program is the real one: the
//! `huddle` program over a mock store, answering at a chain time the test
//! moves, so `Ringing` is what the chain would say. The device's store
//! outlives the notifier's runs, as the app keeps it between launches.
use super::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use ::program::{Env, MockHost, Program};
use huddle::{Huddle, Op, RING_SECONDS};
use view::methods::{
    Change, Changes, ClockTicks, HostSession, NoticeKind, NotifyPost, NotifySeen, Query as Ask,
    Session, StoreGet, StoreSet,
};
use view::testing::TestNotifier;

const ADA: u64 = 7;
const BO: u64 = 8;
const CY: u64 = 9;

/// The reader the host seats: key `0102`, account Bo, on testnet.
fn session() -> Session {
    Session {
        signer: "0102".into(),
        account: Some(BO),
        connected: true,
        chain_id: "testnet-0a1b2c3d".into(),
        ..Session::default()
    }
}

/// The chain as the notifier reads it, and the device's store, all of
/// which outlive the notifier's runs.
#[derive(Clone, Default)]
struct World {
    chain: MockHost,
    /// The chain's time, in seconds.
    now: Rc<Cell<u64>>,
    kept: Rc<RefCell<BTreeMap<String, Vec<u8>>>>,
}

impl World {
    fn env(&self) -> Env {
        Env {
            time: self.now.get() * 1000,
            ..MockHost::env(huddle::PROGRAM)
        }
    }

    /// `op` as `who`, applied.
    fn send(&self, who: u64, op: Op) {
        let ctx = self
            .chain
            .exec(self.env().signed(who.to_be_bytes(), Some(who)));
        if let Err(refusal) = self.chain.attempt(|| Huddle::execute(&ctx, op)) {
            panic!("refused: {refusal}");
        }
    }

    fn notifier(&self) -> TestNotifier<HuddleNotices> {
        let notifier = TestNotifier::new();
        let host = notifier.host();
        let world = self.clone();
        host.handle::<Ask<Huddle>>(move |query| {
            Huddle::query(&world.chain.query(world.env()), query)
        });
        host.handle::<Ask<Identity>>(|query| match query {
            ::identity::Query::Profile { number: ADA } => {
                Ok(::identity::Reply::Profile(Some(::identity::Profile {
                    number: ADA,
                    name: "ada".into(),
                    kind: ::identity::Kind::User,
                })))
            }
            ::identity::Query::Profile { .. } => Ok(::identity::Reply::Profile(None)),
            query => panic!("unexpected identity query: {query:?}"),
        });
        let kept = self.kept.clone();
        host.handle::<StoreGet>(move |key| Ok(kept.borrow().get(&key).cloned()));
        let kept = self.kept.clone();
        host.handle::<StoreSet>(move |(key, value)| {
            match value {
                Some(value) => kept.borrow_mut().insert(key, value),
                None => kept.borrow_mut().remove(&key),
            };
            Ok(())
        });
        notifier
    }

    /// A run for Cy, who knocks, started as a connect starts it.
    fn knocker(&self) -> TestNotifier<HuddleNotices> {
        let mut knocker = self.notifier();
        knocker.start();
        knocker.host().stream::<HostSession>().send(Session {
            account: Some(CY),
            ..session()
        });
        knocker.run_until_parked();
        knocker
    }

    /// A run started as a connect starts it, the reader seated.
    fn start(&self, notifier: &mut TestNotifier<HuddleNotices>) {
        notifier.start();
        notifier.host().stream::<HostSession>().send(session());
        notifier.run_until_parked();
    }

    /// `op` as `who`, and the block that wrote it reaching the notifier.
    fn block(&self, notifier: &mut TestNotifier<HuddleNotices>, who: u64, op: Op) {
        self.send(who, op);
        let keys = vec![huddle::state::CALLS.key(&"call-1".to_owned())];
        notifier
            .host()
            .stream::<Changes<Huddle>>()
            .send(Some(Change { height: 2, keys }));
        notifier.run_until_parked();
    }

    /// The chain's clock at `now`, and the notifier's check ticking once.
    fn tick(&self, notifier: &mut TestNotifier<HuddleNotices>, now: u64) {
        self.now.set(now);
        notifier.host().stream::<ClockTicks>().send(());
        notifier.run_until_parked();
    }
}

fn posts(notifier: &TestNotifier<HuddleNotices>) -> Vec<Notification> {
    notifier.host().requests::<NotifyPost>()
}

fn seen(notifier: &TestNotifier<HuddleNotices>) -> Vec<String> {
    notifier.host().requests::<NotifySeen>()
}

fn start(invited: Vec<u64>) -> Op {
    Op::Start {
        call: "call-1".into(),
        invited,
    }
}

/// A call that rings the reader is one notice: a ring with its `until`,
/// under the call's tag, linking the call, and its end link the call's
/// leave. A block that leaves it ringing
/// rings nothing more; their joining takes it down.
#[test]
fn a_call_ringing_me_is_one_ring_until_i_join() {
    let world = World::default();
    world.now.set(1_000);
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    assert!(posts(&notifier).is_empty(), "nothing rings");

    world.block(&mut notifier, ADA, start(vec![BO, CY]));
    let posted = posts(&notifier);
    assert_eq!(posted.len(), 1, "{posted:?}");
    assert_eq!(
        posted[0],
        Notification {
            title: "ada is calling".into(),
            body: "Huddle with 3 people".into(),
            tag: "call/call-1".into(),
            link: "duck://testnet-0a1b2c3d/huddle/call/call-1".into(),
            kind: NoticeKind::Call,
            until: 1_000 + RING_SECONDS,
            end_link: "duck://testnet-0a1b2c3d/huddle/call/call-1/leave".into(),
        }
    );

    // Cy declines: the call still rings me, and rings nothing new
    world.block(
        &mut notifier,
        CY,
        Op::Decline {
            call: "call-1".into(),
        },
    );
    assert_eq!(posts(&notifier).len(), 1);
    assert!(seen(&notifier).is_empty());

    world.block(
        &mut notifier,
        BO,
        Op::Join {
            call: "call-1".into(),
        },
    );
    assert_eq!(seen(&notifier), ["call/call-1"]);
    assert_eq!(posts(&notifier).len(), 1);
}

/// A ring is taken down whatever stops it: a decline, the caller ending
/// the call, or its time running out, which the check finds.
#[test]
fn a_ring_is_taken_down_when_declined_ended_or_run_out() {
    for stop in ["decline", "end", "time"] {
        let world = World::default();
        let mut notifier = world.notifier();
        world.start(&mut notifier);
        world.block(&mut notifier, ADA, start(vec![BO]));
        assert_eq!(posts(&notifier).len(), 1, "{stop}");
        match stop {
            "decline" => world.block(
                &mut notifier,
                BO,
                Op::Decline {
                    call: "call-1".into(),
                },
            ),
            "end" => world.block(
                &mut notifier,
                ADA,
                Op::End {
                    call: "call-1".into(),
                },
            ),
            _ => {
                world.tick(&mut notifier, RING_SECONDS - 1);
                assert!(seen(&notifier).is_empty(), "still ringing");
                world.tick(&mut notifier, RING_SECONDS);
            }
        }
        assert_eq!(seen(&notifier), ["call/call-1"], "{stop}");
        assert_eq!(posts(&notifier).len(), 1, "{stop}");
    }
}

/// Asked in again after the ring ran out, the reader is rung again, under
/// the same tag, to the new `until`.
#[test]
fn asked_again_rings_again() {
    let world = World::default();
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    world.block(&mut notifier, ADA, start(vec![BO, CY]));
    world.block(
        &mut notifier,
        CY,
        Op::Join {
            call: "call-1".into(),
        },
    );
    world.tick(&mut notifier, RING_SECONDS);
    assert_eq!(seen(&notifier), ["call/call-1"]);
    world.block(
        &mut notifier,
        ADA,
        Op::Invite {
            call: "call-1".into(),
            accounts: vec![BO],
        },
    );
    let posted = posts(&notifier);
    assert_eq!(posted.len(), 2, "{posted:?}");
    assert_eq!(posted[1].until, 2 * RING_SECONDS);
}

/// The app was closed while a call rang, and opened after the ring ran out:
/// the new run rings nothing and takes down what the last one left up. A
/// run that finds a call still ringing that it rang already rings it not
/// again.
#[test]
fn a_fresh_run_never_rings_a_call_that_is_over() {
    let world = World::default();
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    world.block(&mut notifier, ADA, start(vec![BO]));
    assert_eq!(posts(&notifier).len(), 1);

    // relaunched while it still rings
    let mut relaunched = world.notifier();
    world.start(&mut relaunched);
    assert!(posts(&relaunched).is_empty(), "rung already");
    assert!(seen(&relaunched).is_empty());

    // relaunched after it ran out
    world.now.set(RING_SECONDS + 30);
    let mut late = world.notifier();
    world.start(&mut late);
    assert!(posts(&late).is_empty(), "the stale ring");
    assert_eq!(seen(&late), ["call/call-1"]);

    // a device that never rang it, opened late, says nothing at all
    let fresh = World {
        kept: Rc::default(),
        ..world.clone()
    };
    let mut first = fresh.notifier();
    fresh.start(&mut first);
    assert!(posts(&first).is_empty());
    assert!(seen(&first).is_empty());
}

/// The caller is not rung by their own call, and a reader with no account
/// is rung by nothing.
#[test]
fn only_an_invitee_with_an_account_is_rung() {
    let world = World::default();
    let mut notifier = world.notifier();
    notifier.start();
    notifier.host().stream::<HostSession>().send(Session {
        account: Some(ADA),
        ..session()
    });
    notifier.run_until_parked();
    world.block(&mut notifier, ADA, start(vec![BO]));
    assert!(posts(&notifier).is_empty(), "my own call");

    let mut unseated = world.notifier();
    unseated.start();
    unseated.host().stream::<HostSession>().send(Session {
        account: None,
        ..session()
    });
    unseated.run_until_parked();
    assert!(posts(&unseated).is_empty());
    assert!(unseated.host().requests::<Ask<Huddle>>().is_empty());
}

/// The words a ring carries: the caller, and the size of a group call.
#[test]
fn a_ring_names_its_caller_and_a_group() {
    let world = World::default();
    world.send(ADA, start(vec![BO]));
    let one = Huddle::query(
        &world.chain.query(world.env()),
        huddle::Query::Call {
            id: "call-1".into(),
        },
    );
    let Ok(huddle::Reply::Call(Some(call))) = one else {
        panic!("a call");
    };
    let listed = Listed {
        id: "call-1".into(),
        call,
    };
    let rung = notice(&listed, "ada", 45, "testnet-0a1b2c3d");
    assert_eq!(
        (rung.title.as_str(), rung.body.as_str()),
        ("ada is calling", "")
    );
    assert_eq!((rung.kind, rung.until), (NoticeKind::Call, 45));
}

/// A knock at the door of a call I am in is a plain notice, one per
/// knocker, under the knock's own tag, linking the call, until the knock
/// runs out; it is taken down whatever answers it: Let in, Turn away, the
/// knocker's withdrawal or its time running out, which the check finds.
#[test]
fn a_knock_at_my_calls_door_is_a_notice_until_answered() {
    for answer in ["admit", "turn away", "withdraw", "time"] {
        let world = World::default();
        world.now.set(1_000);
        let mut notifier = world.notifier();
        world.start(&mut notifier);
        world.block(&mut notifier, ADA, start(vec![BO]));
        world.block(
            &mut notifier,
            BO,
            Op::Join {
                call: "call-1".into(),
            },
        );
        assert_eq!(seen(&notifier), ["call/call-1"], "{answer}: my ring went");
        world.block(
            &mut notifier,
            CY,
            Op::Knock {
                call: "call-1".into(),
            },
        );
        let posted = posts(&notifier);
        assert_eq!(posted.len(), 2, "{answer}: {posted:?}");
        assert_eq!(
            posted[1],
            Notification {
                title: "account 9 is asking to join".into(),
                body: String::new(),
                tag: "knock/call-1/9".into(),
                link: "duck://testnet-0a1b2c3d/huddle/call/call-1".into(),
                kind: NoticeKind::Notice,
                until: 1_000 + huddle::KNOCK_SECONDS,
                end_link: String::new(),
            }
        );
        match answer {
            "admit" => world.block(
                &mut notifier,
                BO,
                Op::Admit {
                    call: "call-1".into(),
                    account: CY,
                },
            ),
            "turn away" => world.block(
                &mut notifier,
                ADA,
                Op::TurnAway {
                    call: "call-1".into(),
                    account: CY,
                },
            ),
            "withdraw" => world.block(
                &mut notifier,
                CY,
                Op::Leave {
                    call: "call-1".into(),
                },
            ),
            _ => {
                world.tick(&mut notifier, 1_000 + huddle::KNOCK_SECONDS - 1);
                assert_eq!(seen(&notifier).len(), 1, "{answer}: still waiting");
                world.tick(&mut notifier, 1_000 + huddle::KNOCK_SECONDS);
            }
        }
        assert_eq!(
            seen(&notifier),
            ["call/call-1", "knock/call-1/9"],
            "{answer}"
        );
        assert_eq!(posts(&notifier).len(), 2, "{answer}: nothing new");
    }
}

/// The knocker hears nothing of their knock, and let in, no ring (they
/// asked): one plain notice that they were let in. Nor does one rung into
/// the call but not in it hear the knock.
#[test]
fn the_knocker_hears_only_that_they_were_let_in() {
    let world = World::default();
    world.now.set(1_000);
    let mut knocker = world.knocker();
    let mut rung = world.notifier();
    world.start(&mut rung);
    world.block(&mut knocker, ADA, start(vec![BO]));
    world.block(
        &mut knocker,
        CY,
        Op::Knock {
            call: "call-1".into(),
        },
    );
    assert!(posts(&knocker).is_empty(), "{:?}", posts(&knocker));
    world.block(
        &mut knocker,
        ADA,
        Op::Admit {
            call: "call-1".into(),
            account: CY,
        },
    );
    assert_eq!(
        posts(&knocker),
        [Notification {
            title: "You were let into ada's call".into(),
            body: String::new(),
            tag: "in/call-1".into(),
            link: "duck://testnet-0a1b2c3d/huddle/call/call-1".into(),
            kind: NoticeKind::Notice,
            until: 1_000 + huddle::WAIT_SECONDS,
            end_link: String::new(),
        }]
    );
    // Bo, rung and not in: the ring alone
    rung.host().stream::<Changes<Huddle>>().send(Some(Change {
        height: 3,
        keys: vec![huddle::state::CALLS.key(&"call-1".to_owned())],
    }));
    rung.run_until_parked();
    let posted = posts(&rung);
    assert_eq!(posted.len(), 1, "{posted:?}");
    assert_eq!(posted[0].kind, NoticeKind::Call);
}

/// Let in is taken down when the knocker joins, withdraws or declines, or
/// the wait from being let in runs out.
#[test]
fn let_in_is_taken_down_when_joined_withdrawn_declined_or_run_out() {
    for end in ["join", "withdraw", "decline", "time"] {
        let world = World::default();
        world.now.set(1_000);
        let mut knocker = world.knocker();
        world.block(&mut knocker, ADA, start(vec![]));
        let call = || "call-1".to_owned();
        world.block(&mut knocker, CY, Op::Knock { call: call() });
        world.block(
            &mut knocker,
            ADA,
            Op::Admit {
                call: call(),
                account: CY,
            },
        );
        assert_eq!(posts(&knocker).len(), 1, "{end}");
        match end {
            "join" => world.block(&mut knocker, CY, Op::Join { call: call() }),
            "withdraw" => world.block(&mut knocker, CY, Op::Leave { call: call() }),
            "decline" => world.block(&mut knocker, CY, Op::Decline { call: call() }),
            _ => {
                // Ada's view keeps the call waiting: the let in runs out alone
                world.now.set(1_000 + huddle::WAIT_SECONDS - 1);
                world.send(ADA, Op::Join { call: call() });
                world.tick(&mut knocker, 1_000 + huddle::WAIT_SECONDS - 1);
                assert!(seen(&knocker).is_empty(), "{end}: still let in");
                world.tick(&mut knocker, 1_000 + huddle::WAIT_SECONDS);
            }
        }
        assert_eq!(seen(&knocker), ["in/call-1"], "{end}");
        assert_eq!(posts(&knocker).len(), 1, "{end}: nothing new");
    }
}
