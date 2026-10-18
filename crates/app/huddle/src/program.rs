//! The program (its name and types, which a view names it by) and its
//! dispatch: every op and every query, each handed to its function in
//! `rules.rs` or `queries.rs`.
use ::program::{Error, ExecCtx, Program, QueryCtx};

use crate::queries;
use crate::rules::{admit, decline, end, invite, join, knock, leave, start, turn_away};
use crate::{Op, PROGRAM, Query, Reply, seconds};

pub struct Huddle;

impl Program for Huddle {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        // a call is between accounts: a key that holds none calls no one
        let me = ctx.sender_account()?;
        let now = seconds(&ctx.env());
        match op {
            Op::Start { call, invited } => start(ctx, me, call, invited, now),
            Op::Invite { call, accounts } => invite(ctx, me, call, accounts, now),
            Op::Join { call } => join(ctx, me, call, now),
            Op::Leave { call } => leave(ctx, me, call, now),
            Op::Decline { call } => decline(ctx, me, call),
            Op::End { call } => end(ctx, me, call, now),
            Op::Knock { call } => knock(ctx, me, call, now),
            Op::Admit { call, account } => admit(ctx, me, call, account, now),
            Op::TurnAway { call, account } => turn_away(ctx, me, call, account),
        }
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        let now = seconds(&ctx.env());
        Ok(match query {
            Query::Call { id } => Reply::Call(queries::call(ctx, &id)?),
            Query::Mine { account } => Reply::Calls(queries::mine(ctx, account, now)?),
            Query::Ringing { account } => Reply::Calls(queries::ringing(ctx, account, now)?),
            Query::Knocks { call } => Reply::Knocks(queries::knocks_at(ctx, &call, now)?),
            Query::Doors { account } => Reply::Doors(queries::doors(ctx, account, now)?),
        })
    }
}

#[cfg(feature = "program")]
::program::export!(Huddle);
