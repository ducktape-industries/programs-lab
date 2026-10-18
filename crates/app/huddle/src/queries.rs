//! One function per [`Query`](crate::Query), each named by [`Huddle::query`](crate::Huddle)'s
//! match and each a bounded read of the tables in `state.rs`, at the
//! chain's time `now` (seconds).
use ::program::{Error, QueryCtx, corrupt};

use crate::rules::knocks;
use crate::state::{CALLS, KNOCKS, MINE, WAITS};
use crate::{Answer, Call, Door, Knocker, Knocking, Listed};

/// The most of an account's calls, newest first, a list reads.
// ponytail: a call that went over with no write (no one answered, or
// everyone left without Leave) keeps its row in MINE; past this many newer
// ones an open call is not listed. Prune MINE in the op that finds a call
// over if lists ever miss one.
const NEWEST: u64 = 32;

pub(crate) fn call(ctx: &QueryCtx, id: &str) -> Result<Option<Call>, Error> {
    CALLS.get(ctx, &id.to_owned())
}

pub(crate) fn mine(ctx: &QueryCtx, account: u64, now: u64) -> Result<Vec<Listed>, Error> {
    listed(ctx, account, |id, call| {
        let waits_until = WAITS.get(ctx, &id.to_owned())?;
        Ok(!call.over(&knocks(ctx, id)?, waits_until, now))
    })
}

pub(crate) fn ringing(ctx: &QueryCtx, account: u64, now: u64) -> Result<Vec<Listed>, Error> {
    listed(ctx, account, |id, call| {
        if !call.rings(account, now) {
            return Ok(false);
        }
        // let in at the door, not called: nothing rings them
        let knock = KNOCKS.get(ctx, &(id.to_owned(), account))?;
        Ok(!knock.is_some_and(|knock| knock.answer == Some(Answer::Admitted)))
    })
}

pub(crate) fn knocks_at(
    ctx: &QueryCtx,
    id: &str,
    now: u64,
) -> Result<Option<Vec<Knocking>>, Error> {
    if !CALLS.has(ctx, &id.to_owned()) {
        return Ok(None);
    }
    let knocking = knocks(ctx, id)?
        .into_iter()
        .map(|knocker| Knocking {
            account: knocker.account,
            standing: knocker.standing(now),
        })
        .collect();
    Ok(Some(knocking))
}

pub(crate) fn doors(ctx: &QueryCtx, account: u64, now: u64) -> Result<Vec<Door>, Error> {
    let mut doors = Vec::new();
    for Listed { id, call } in listed(ctx, account, |_, call| Ok(call.joined.contains(&account)))? {
        if call.ended.is_some() {
            continue;
        }
        let waiting: Vec<Knocker> = knocks(ctx, &id)?
            .into_iter()
            .filter(|knocker| knocker.waits(now))
            .collect();
        if !waiting.is_empty() {
            doors.push(Door { call: id, waiting });
        }
    }
    Ok(doors)
}

/// `account`'s newest calls that `keep` keeps.
fn listed(
    ctx: &QueryCtx,
    account: u64,
    keep: impl Fn(&str, &Call) -> Result<bool, Error>,
) -> Result<Vec<Listed>, Error> {
    let newest = MINE.prefix_of(&account).reverse().limit(NEWEST);
    let mut calls = Vec::new();
    for (_, _, id) in MINE.scan(ctx, newest)? {
        // a call is never deleted, so its index row never outlives it
        let call = CALLS
            .get(ctx, &id)?
            .ok_or_else(|| corrupt(MINE.prefix(), id.as_bytes(), "an index row with no call"))?;
        if keep(&id, &call)? {
            calls.push(Listed { id, call });
        }
    }
    Ok(calls)
}
