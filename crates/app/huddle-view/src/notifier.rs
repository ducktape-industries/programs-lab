//! What huddle hands the host's notification centre, decided here and
//! nowhere else: a call ringing the reader, a knock at the door of a call
//! the reader is in, and a call that let the reader in at its door.
//!
//! A ring's title is the caller (`Ada is calling`), its body how many are
//! asked in when more than two, its tag the call (`call/<id>`), its link
//! the call (`duck://<chain>/huddle/call/<id>`, which the view answers at
//! once), its kind [`NoticeKind::Call`] (a carrier rings: a VoIP push on a
//! phone, a banner with Answer and Decline on a desktop), its `until` the
//! end of the reader's ring, in the chain's seconds, and its end link the
//! call's leave (`duck://<chain>/huddle/call/<id>/leave`, which the view
//! leaves that call by): what a carrier opens when the person ends the
//! answered call from the system's own call screen.
//!
//! A knock is a plain notice ([`NoticeKind::Notice`]: shown and logged,
//! nothing rings) to each person in the call, one per knocker: its title
//! the knocker (`Bo is asking to join`), its body empty, its tag the knock
//! (`knock/<call>/<account>`: a knock answered or run out is withdrawn
//! alone, and never takes a ring's tag down), its link the call (the view
//! in the room shows the knock with Let in and Turn away), its `until` the
//! knock's run-out (a carrier drops one that arrives late).
//!
//! Let in is a plain notice to the knocker, for when their view is not
//! open to join at once (a phone's app put away): its title the call
//! (`You were let into Ada's call`: the call is the caller's, and the
//! program keeps no record of who let them in), its body empty, its tag
//! the call's door (`in/<call>`), its link the call (the view joins it at
//! once), its `until` [`huddle::WAIT_SECONDS`] from being let in. It is
//! withdrawn when they join, withdraw or decline, or the call is over.
//! Turned away or run out, the knocker gets nothing: their open view says
//! so.
//!
//! This is huddle's NOTIFIER: the host runs it once per network, whether or
//! not a Huddle pane is open. It asks the program which calls ring the
//! reader ([`huddle::Query::Ringing`]), which knocks wait at the doors of
//! the calls they are in ([`huddle::Query::Doors`]) and which calls let
//! them in ([`huddle::Query::LetIn`]) when a block writes a call or a
//! door, and every five seconds while it has a notice out, so one that
//! runs out is withdrawn (`notify.seen`) as one that is answered, declined
//! or ended is. The program answers at the chain's time, so a run catching
//! up after a while away never rings a call that is over, nor says a knock
//! that ran out.
//!
//! What it posted it keeps under `rung/<signer>`: each tag with the `until`
//! it was posted to. A call rung again (asked in again, a new `until`)
//! rings again; a run that starts withdraws what an earlier run left up
//! that stands no more.
use std::collections::BTreeMap;

use huddle::{Door, KNOCK_SECONDS, Listed, WAIT_SECONDS};
use identity::Identity;
use view::prelude::*;
use view::{Notifier, store};

use crate::words;

/// Where the notifier keeps what it posted.
fn rung_key(signer: &str) -> String {
    format!("rung/{signer}")
}

/// How often a ring or a knock out is checked, in milliseconds: a carrier
/// also stops a ring at its `until`, so this only takes the notice down.
const CHECK_MS: i64 = 5_000;

/// Huddle's notifier: who reads, what it posted, and the check that runs
/// while something is out.
#[derive(Default)]
pub struct HuddleNotices {
    session: Session,
    /// Moves with every new reader: what lands for an earlier one is dropped.
    run: u64,
    /// Each notice out, by its tag, with the `until` it was posted to;
    /// `None` until read off the store.
    posted: Option<BTreeMap<String, u64>>,
    /// Moves with every ask: an answer to an older ask is dropped, so a
    /// slow one never rings a call a newer one saw stop.
    asked: u64,
    /// The check every [`CHECK_MS`], while anything is out.
    clock: Option<Task<()>>,
}

impl Notifier for HuddleNotices {
    const NAME: &'static str = words::HUDDLE;
    const CAPABILITIES: &'static [Capability] = &[
        Capability::Program,
        Capability::Host,
        Capability::Notify,
        Capability::Store,
        Capability::Clock,
    ];
    const TARGETS: &'static [&'static str] = &[huddle::PROGRAM, identity::PROGRAM];

    fn new(cx: &mut Context<Self>) -> Self {
        cx.follow::<HostSession>((), |notices, session, cx| match session {
            Ok(session) => notices.session_changed(session, cx),
            Err(refusal) => log_refused(cx, "the session", &refusal),
        })
        .detach();
        // a block that wrote a call of the reader's, or a door, asks again
        cx.follow::<Changes<huddle::Huddle>>((), |notices, change, cx| match change {
            Ok(change) => {
                let Some(account) = notices.session.account else {
                    return;
                };
                let ringing = huddle::ask::Ringing { account };
                let doors = huddle::ask::Doors { account };
                let let_in = huddle::ask::LetIn { account };
                if change.is_none_or(|change| {
                    change.touches::<huddle::Huddle, _>(&ringing)
                        || change.touches::<huddle::Huddle, _>(&doors)
                        || change.touches::<huddle::Huddle, _>(&let_in)
                }) {
                    notices.ask(cx);
                }
            }
            Err(refusal) => log_refused(cx, "huddle's blocks", &refusal),
        })
        .detach();
        Self::default()
    }
}

fn log_refused(cx: &mut Context<HuddleNotices>, what: &str, refusal: &Error) {
    cx.host().log(format!(
        "{}: {what} refused: {refusal}",
        HuddleNotices::NAME
    ));
}

/// What is to be out now: each notice, by its tag, with its `until`.
type Postings = Vec<(String, u64, Notification)>;

impl HuddleNotices {
    fn session_changed(&mut self, next: Session, cx: &mut Context<Self>) {
        let reader = (&next.signer, &next.chain_id, next.account, next.connected);
        let was = (
            &self.session.signer,
            &self.session.chain_id,
            self.session.account,
            self.session.connected,
        );
        if reader == was {
            self.session = next;
            return;
        }
        // another reader, network or account: nothing known carries over
        *self = Self {
            session: next,
            run: self.run + 1,
            asked: self.asked,
            ..Self::default()
        };
        if !self.session.connected
            || self.session.signer.is_empty()
            || self.session.account.is_none()
        {
            return;
        }
        let kept = store::get::<BTreeMap<String, u64>>(&cx.host(), &rung_key(&self.session.signer));
        let run = self.run;
        cx.land(kept, move |notices, kept, cx| {
            if notices.run != run {
                return;
            }
            let kept = kept.unwrap_or_else(|refusal| {
                log_refused(cx, "what was posted", &refusal);
                None
            });
            notices.posted = Some(kept.unwrap_or_default());
            notices.ask(cx);
        })
        .detach();
    }

    /// What stands for the reader now: the calls ringing them and the
    /// calls that let them in, each with its caller's name, and the knocks
    /// at their doors, each with the knocker's; once what was posted is
    /// read.
    fn ask(&mut self, cx: &mut Context<Self>) {
        let (Some(account), Some(_)) = (self.session.account, &self.posted) else {
            return;
        };
        self.asked += 1;
        let (run, asked) = (self.run, self.asked);
        let postings = postings(cx.host(), account, self.session.chain_id.clone());
        cx.land(postings, move |notices, postings, cx| {
            if (notices.run, notices.asked) != (run, asked) {
                return;
            }
            match postings {
                Ok(postings) => notices.settle(postings, cx),
                Err(refusal) => log_refused(cx, "what rings and knocks", &refusal),
            }
        })
        .detach();
    }

    /// `postings` is what is to be out now: a notice not posted yet, or
    /// posted to another `until`, is posted; one posted that is not here
    /// any more is withdrawn. The check runs while anything is out.
    fn settle(&mut self, postings: Postings, cx: &mut Context<Self>) {
        let Some(posted) = &mut self.posted else {
            return;
        };
        let before = posted.clone();
        for (tag, until, notice) in &postings {
            if posted.insert(tag.clone(), *until) != Some(*until) {
                cx.host().notify::<NotifyPost>(notice.clone());
            }
        }
        posted.retain(|tag, _| {
            let stands = postings.iter().any(|(posting, ..)| posting == tag);
            if !stands {
                cx.host().notify::<NotifySeen>(tag.clone());
            }
            stands
        });
        if *posted != before {
            store::set(&cx.host(), &rung_key(&self.session.signer), Some(&*posted));
        }
        let out = !posted.is_empty();
        match (out, self.clock.is_some()) {
            (true, false) => {
                self.clock = Some(cx.follow::<ClockTicks>(
                    CHECK_MS,
                    |notices, tick, cx| match tick {
                        Ok(()) => notices.ask(cx),
                        Err(refusal) => log_refused(cx, "the clock", &refusal),
                    },
                ));
            }
            (false, true) => self.clock = None,
            _ => {}
        }
    }
}

/// The notices to be out for `account` on `chain`: a ring for each call
/// ringing them, a notice for each knock at the doors of the calls they
/// are in and for each call that let them in, each named by identity (the
/// number where it names none).
async fn postings(host: Host, account: u64, chain: String) -> Result<Postings, Error> {
    let mut postings = Vec::new();
    for listed in host.query(huddle::ask::Ringing { account }).await? {
        let Some(until) = listed.call.ring_until(account) else {
            continue;
        };
        let caller = name(&host, listed.call.caller).await;
        postings.push((
            tag(&listed.id),
            until,
            notice(&listed, &caller, until, &chain),
        ));
    }
    for Door { call, waiting } in host.query(huddle::ask::Doors { account }).await? {
        for knocker in waiting {
            let who = name(&host, knocker.account).await;
            let until = knocker.knock.at + KNOCK_SECONDS;
            postings.push((
                knock_tag(&call, knocker.account),
                until,
                knock(&call, knocker.account, &who, until, &chain),
            ));
        }
    }
    for listed in host.query(huddle::ask::LetIn { account }).await? {
        // let in is asked in, from when they were let in
        let invitee = listed.call.invited.iter();
        let Some(at) = invitee
            .filter(|invitee| invitee.account == account)
            .map(|invitee| invitee.at)
            .next()
        else {
            continue;
        };
        let until = at + WAIT_SECONDS;
        let caller = name(&host, listed.call.caller).await;
        postings.push((
            let_in_tag(&listed.id),
            until,
            let_in(&listed.id, &caller, until, &chain),
        ));
    }
    Ok(postings)
}

/// What identity calls `account`, or their number.
async fn name(host: &Host, account: u64) -> String {
    let profile = host.ask::<Query<Identity>>(::identity::Query::Profile { number: account });
    match profile.await {
        Ok(::identity::Reply::Profile(Some(profile))) => profile.name,
        _ => words::account(account),
    }
}

/// The tag a call's ring goes under.
pub(crate) fn tag(id: &str) -> String {
    format!("call/{id}")
}

/// The tag `account`'s knock at call `id` goes under.
pub(crate) fn knock_tag(id: &str, account: u64) -> String {
    format!("knock/{id}/{account}")
}

/// The ring of `listed` for its invitee, from `caller`, until `until`.
pub(crate) fn notice(listed: &Listed, caller: &str, until: u64, chain: &str) -> Notification {
    Notification {
        title: words::calling(caller),
        body: words::huddle_with(listed.call.people().count()),
        tag: tag(&listed.id),
        // a notice without a link is one the centre cannot open, not a bad one
        link: ducklink::mint(chain, huddle::PROGRAM, &["call", &listed.id]).unwrap_or_default(),
        kind: NoticeKind::Call,
        until,
        end_link: ducklink::mint(chain, huddle::PROGRAM, &["call", &listed.id, "leave"])
            .unwrap_or_default(),
    }
}

/// The tag a call that let the reader in goes under.
pub(crate) fn let_in_tag(id: &str) -> String {
    format!("in/{id}")
}

/// Call `id`, `caller`'s, let the reader in, until `until`.
pub(crate) fn let_in(id: &str, caller: &str, until: u64, chain: &str) -> Notification {
    Notification {
        title: words::let_into(caller),
        body: String::new(),
        tag: let_in_tag(id),
        link: ducklink::mint(chain, huddle::PROGRAM, &["call", id]).unwrap_or_default(),
        kind: NoticeKind::Notice,
        until,
        end_link: String::new(),
    }
}

/// `who`'s (account `knocker`) knock at call `id`, to someone in it,
/// until it runs out.
pub(crate) fn knock(id: &str, knocker: u64, who: &str, until: u64, chain: &str) -> Notification {
    Notification {
        title: words::asking_to_join(who),
        body: String::new(),
        tag: knock_tag(id, knocker),
        link: ducklink::mint(chain, huddle::PROGRAM, &["call", id]).unwrap_or_default(),
        kind: NoticeKind::Notice,
        until,
        end_link: String::new(),
    }
}

#[cfg(test)]
mod tests;
