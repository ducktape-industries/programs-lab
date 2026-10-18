//! What each [`Op`](crate::Op) checks and writes: it reads first, then
//! writes, so a refused op leaves the store as it found it. Join and Knock
//! are refused once a call is over ([`Call::over`]); Invite, which rings
//! again, and Leave, Decline and End, which take people out, are refused
//! only once it has ended, so a call that went over with no write can still
//! be rung again by the one left in it, or closed. Admit and TurnAway
//! answer a knock that waits, so they are refused once it ran out, and
//! once the call has ended.
use ::program::live::{grant, revoke};
use ::program::{
    Error, ExecCtx, already_exists, capacity, invalid, not_found, unauthorized, wrong_state,
};

use crate::state::{CALLS, KNOCKS, MINE, WAITS};
use crate::{
    Answer, Call, Invitee, Knock, Knocker, MAX_CALL_ID, MAX_PEOPLE, Standing, WAIT_SECONDS, seat,
};

pub(crate) fn start(
    ctx: &ExecCtx,
    me: u64,
    id: String,
    invited: Vec<u64>,
    now: u64,
) -> Result<(), Error> {
    valid_id(&id)?;
    if CALLS.has(ctx, &id) {
        return Err(already_exists(format!("there is a call {id} already")));
    }
    let mut call = Call {
        caller: me,
        invited: Vec::new(),
        joined: vec![me],
        declined: Vec::new(),
        started: now,
        ended: None,
    };
    ask_in(&mut call, invited, now)?;
    CALLS.put(ctx, &id, &call);
    // alone or not, the call waits for knocks from its start
    WAITS.put(ctx, &id, &(now + WAIT_SECONDS));
    for account in call.people() {
        sit(ctx, &id, &call, account);
    }
    Ok(())
}

pub(crate) fn invite(
    ctx: &ExecCtx,
    me: u64,
    id: String,
    accounts: Vec<u64>,
    now: u64,
) -> Result<(), Error> {
    let mut call = open(ctx, &id)?;
    in_the_call(&call, me, "asks people into it")?;
    // a call no one answered is over for a Join, not for its caller still
    // in the room: their Invite rings again
    not_ended(&call, &id)?;
    let asked = ask_in(&mut call, accounts, now)?;
    if asked.is_empty() {
        return Ok(());
    }
    CALLS.put(ctx, &id, &call);
    for account in asked {
        sit(ctx, &id, &call, account);
    }
    Ok(())
}

pub(crate) fn join(ctx: &ExecCtx, me: u64, id: String, now: u64) -> Result<(), Error> {
    let mut call = open(ctx, &id)?;
    if !call.asked(me) {
        return Err(unauthorized(format!("you were not asked into call {id}")));
    }
    if over(ctx, &id, &call, now)? {
        return Err(wrong_state(format!("call {id} is over")));
    }
    if !call.joined.contains(&me) {
        call.joined.push(me);
    }
    call.declined.retain(|account| *account != me);
    CALLS.put(ctx, &id, &call);
    sit(ctx, &id, &call, me);
    // in: the knock that let them in has done its work
    KNOCKS.remove(ctx, &(id, me));
    Ok(())
}

pub(crate) fn leave(ctx: &ExecCtx, me: u64, id: String, now: u64) -> Result<(), Error> {
    let mut call = open(ctx, &id)?;
    not_ended(&call, &id)?;
    if !call.joined.contains(&me) {
        // not in it: a knocker withdraws their knock, and one let in
        // already says no as a Decline does, so nothing rings them;
        // anyone else has nothing to leave
        let Some(knock) = KNOCKS.get(ctx, &(id.clone(), me))? else {
            return Err(wrong_state(format!("you are not in call {id}")));
        };
        if knock.answer == Some(Answer::Admitted) {
            decline(ctx, me, id.clone())?;
        }
        KNOCKS.remove(ctx, &(id, me));
        return Ok(());
    }
    call.joined.retain(|account| *account != me);
    if call.joined.is_empty() {
        close(ctx, &id, call, now)?;
    } else {
        CALLS.put(ctx, &id, &call);
        stand(ctx, &id, &call, me);
    }
    Ok(())
}

pub(crate) fn decline(ctx: &ExecCtx, me: u64, id: String) -> Result<(), Error> {
    let mut call = open(ctx, &id)?;
    if call.ring_until(me).is_none() {
        return Err(unauthorized(format!("you were not invited to call {id}")));
    }
    not_ended(&call, &id)?;
    if call.joined.contains(&me) {
        return Err(wrong_state(format!("you are in call {id}: leave it")));
    }
    if !call.declined.contains(&me) {
        call.declined.push(me);
    }
    CALLS.put(ctx, &id, &call);
    stand(ctx, &id, &call, me);
    Ok(())
}

pub(crate) fn end(ctx: &ExecCtx, me: u64, id: String, now: u64) -> Result<(), Error> {
    let call = open(ctx, &id)?;
    if call.caller != me {
        return Err(unauthorized(format!("only its caller ends call {id}")));
    }
    not_ended(&call, &id)?;
    close(ctx, &id, call, now)
}

pub(crate) fn knock(ctx: &ExecCtx, me: u64, id: String, now: u64) -> Result<(), Error> {
    let call = open(ctx, &id)?;
    if call.asked(me) {
        return Err(wrong_state(format!(
            "you were asked into call {id}: join it"
        )));
    }
    if over(ctx, &id, &call, now)? {
        return Err(wrong_state(format!("call {id} is over")));
    }
    // the door holds as many as the call has room for: a knock past that
    // could never be let in
    let waiting = knocks(ctx, &id)?
        .into_iter()
        .filter(|knocker| knocker.account != me && knocker.waits(now))
        .count();
    if call.people().count() + waiting >= MAX_PEOPLE {
        return Err(capacity(format!(
            "a call holds at most {MAX_PEOPLE} people, and as many wait at its door"
        )));
    }
    KNOCKS.put(
        ctx,
        &(id.clone(), me),
        &Knock {
            at: now,
            answer: None,
        },
    );
    WAITS.put(ctx, &id, &(now + WAIT_SECONDS));
    Ok(())
}

pub(crate) fn admit(
    ctx: &ExecCtx,
    me: u64,
    id: String,
    account: u64,
    now: u64,
) -> Result<(), Error> {
    let mut call = open(ctx, &id)?;
    in_the_call(&call, me, "lets people into it")?;
    not_ended(&call, &id)?;
    let mut knock = knocking(ctx, &id, account, now)?;
    ask_in(&mut call, vec![account], now)?;
    knock.answer = Some(Answer::Admitted);
    CALLS.put(ctx, &id, &call);
    sit(ctx, &id, &call, account);
    KNOCKS.put(ctx, &(id, account), &knock);
    Ok(())
}

pub(crate) fn turn_away(
    ctx: &ExecCtx,
    me: u64,
    id: String,
    account: u64,
    now: u64,
) -> Result<(), Error> {
    let call = open(ctx, &id)?;
    in_the_call(&call, me, "turns people away from it")?;
    not_ended(&call, &id)?;
    let mut knock = knocking(ctx, &id, account, now)?;
    knock.answer = Some(Answer::TurnedAway);
    KNOCKS.put(ctx, &(id, account), &knock);
    Ok(())
}

/// A call id as a room and a link segment take it: 1 to [`MAX_CALL_ID`]
/// bytes of ASCII letters, digits, `-` and `_` (what `host.id` mints).
fn valid_id(id: &str) -> Result<(), Error> {
    let fits = (1..=MAX_CALL_ID).contains(&id.len());
    let plain = id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    match fits && plain {
        true => Ok(()),
        false => Err(invalid(format!(
            "a call id is 1 to {MAX_CALL_ID} letters, digits, - and _"
        ))),
    }
}

/// The call `id`, ended or not.
fn open(ctx: &ExecCtx, id: &str) -> Result<Call, Error> {
    CALLS
        .get(ctx, &id.to_owned())?
        .ok_or_else(|| not_found(format!("there is no call {id}")))
}

fn not_ended(call: &Call, id: &str) -> Result<(), Error> {
    match call.ended {
        Some(_) => Err(wrong_state(format!("call {id} has ended"))),
        None => Ok(()),
    }
}

/// Only someone in the call `does` what Invite, Admit and TurnAway do.
fn in_the_call(call: &Call, me: u64, does: &str) -> Result<(), Error> {
    match call.joined.contains(&me) {
        true => Ok(()),
        false => Err(unauthorized(format!("only someone in a call {does}"))),
    }
}

/// Whether `call` is over at `now`, its door read ([`Call::over`]).
fn over(ctx: &ExecCtx, id: &str, call: &Call, now: u64) -> Result<bool, Error> {
    let waits_until = WAITS.get(ctx, &id.to_owned())?;
    Ok(call.over(&knocks(ctx, id)?, waits_until, now))
}

/// Every knock at `id`'s door.
pub(crate) fn knocks(ctx: &::program::QueryCtx, id: &str) -> Result<Vec<Knocker>, Error> {
    let rows = KNOCKS.scan(ctx, KNOCKS.prefix_of(&id.to_owned()))?;
    Ok(rows
        .into_iter()
        .map(|((_, account), knock)| Knocker { account, knock })
        .collect())
}

/// `account`'s knock at `id`'s door, waiting for an answer at `now`: one
/// that ran out asks no more, so no one lets them in unasked.
fn knocking(ctx: &ExecCtx, id: &str, account: u64, now: u64) -> Result<Knock, Error> {
    let knock = KNOCKS
        .get(ctx, &(id.to_owned(), account))?
        .ok_or_else(|| not_found(format!("account {account} is not knocking at call {id}")))?;
    match (Knocker { account, knock }).standing(now) {
        Standing::Waiting => Ok(knock),
        Standing::RanOut => Err(wrong_state(format!(
            "account {account}'s knock at call {id} ran out"
        ))),
        Standing::Admitted | Standing::TurnedAway => Err(wrong_state(format!(
            "account {account}'s knock at call {id} was answered"
        ))),
    }
}

/// Asks each of `accounts` into `call` at `now`, the caller and those in
/// it already aside, each once: one asked before is rung again from now,
/// one who declined is asked again. Returns who was asked; refused, past
/// [`MAX_PEOPLE`], with `call` left half-changed (the caller drops it).
fn ask_in(call: &mut Call, accounts: Vec<u64>, now: u64) -> Result<Vec<u64>, Error> {
    let mut asked = Vec::new();
    for account in accounts {
        if account == call.caller || call.joined.contains(&account) || asked.contains(&account) {
            continue;
        }
        asked.push(account);
        call.declined.retain(|declined| *declined != account);
        match call
            .invited
            .iter_mut()
            .find(|invitee| invitee.account == account)
        {
            Some(invitee) => invitee.at = now,
            None => call.invited.push(Invitee { account, at: now }),
        }
    }
    if call.people().count() > MAX_PEOPLE {
        return Err(capacity(format!(
            "a call holds at most {MAX_PEOPLE} people"
        )));
    }
    Ok(asked)
}

/// Ends `call` at `now`: every row in its room goes, and its door's.
fn close(ctx: &ExecCtx, id: &str, mut call: Call, now: u64) -> Result<(), Error> {
    call.ended = Some(now);
    CALLS.put(ctx, &id.to_owned(), &call);
    for account in call.people() {
        stand(ctx, id, &call, account);
    }
    for knocker in knocks(ctx, id)? {
        KNOCKS.remove(ctx, &(id.to_owned(), knocker.account));
    }
    WAITS.remove(ctx, &id.to_owned());
    Ok(())
}

/// `account` has a row in `call`'s room and the call among theirs.
fn sit(ctx: &ExecCtx, id: &str, call: &Call, account: u64) {
    grant(ctx, id, Some(account), seat());
    MINE.insert(ctx, &(account, call.started, id.to_owned()));
}

/// `account`'s row in `call`'s room taken back, and the call off theirs.
fn stand(ctx: &ExecCtx, id: &str, call: &Call, account: u64) {
    revoke(ctx, id, Some(account));
    MINE.remove(ctx, &(account, call.started, id.to_owned()));
}
