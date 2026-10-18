//! The tables, beside the live grant rows (`live/<call>/<account>`) the
//! rules write through `::program::live`.
use store::{Map, Set};

use crate::{Call, Knock};

/// Each call, by its id.
pub const CALLS: Map<String, Call> = Map::new("call/");

/// Each person's calls: `(account, started, id)`, so a scan of an
/// account's prefix backwards reads its newest call first. A person is
/// here from being asked in (or joining) until they decline or leave, or
/// the call ends.
pub const MINE: Set<(u64, u64, String)> = Set::new("mine/");

/// Each knock at a call's door, by `(call, account)`: one per knocker,
/// knocked again in place; kept with its answer until the knocker knocks
/// again, withdraws or joins, so their view reads what was answered. The
/// call's end takes them all.
pub const KNOCKS: Map<(String, u64), Knock> = Map::new("knock/");

/// Until when each call waits for knocks, in the chain's seconds: written at
/// its start and at each knock, [`WAIT_SECONDS`](crate::WAIT_SECONDS) on.
/// A call started before there were knocks has no row, and is over as it
/// was.
pub const WAITS: Map<String, u64> = Map::new("wait/");
