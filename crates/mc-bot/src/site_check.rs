//! The live half of [`crate::survey`]: wait for the chunks, then read the blocks from them.

use crate::builder::PlayClient;
use crate::survey::{self, Area, Props, Survey};
use crate::world::BlockPos;
use std::io;
use std::time::{Duration, Instant};

/// How long the server may take to send the chunks around the site.
pub const CHUNK_WAIT: Duration = Duration::from_secs(20);

/// Name and properties of the block the server shows at `p`; `None` while unknown.
pub fn block_at(bot: &PlayClient, p: BlockPos) -> Option<(String, Props)> {
    let id = bot.chunks.state(p)?;
    // a state id outside the release's table is reported as an unknown (so non-natural) block
    let known = crate::verify::decode(bot.ver.tables, id);
    Some(known.map_or_else(|| (format!("state_{id}"), Props::new()), |(n, p)| (n.to_string(), p)))
}

/// Pump packets until every chunk column in `columns` is loaded; false when `limit` passes first.
pub fn wait_for_chunks(bot: &mut PlayClient, columns: &[(i32, i32)], limit: Duration) -> io::Result<bool> {
    let end = Instant::now() + limit;
    loop {
        let missing = columns.iter().any(|&(x, z)| !bot.chunks.is_loaded(x * 16, z * 16));
        if !missing || bot.kicked.is_some() || Instant::now() >= end {
            return Ok(!missing);
        }
        bot.pump_once()?;
    }
}

/// Survey `area` as the server shows it now.
pub fn check(bot: &mut PlayClient, area: &Area, limit: Duration) -> io::Result<Survey> {
    let columns: Vec<(i32, i32)> = area.chunks().into_iter().collect();
    wait_for_chunks(bot, &columns, limit)?;
    // updates still on their way
    bot.idle(Duration::from_millis(300))?;
    Ok(survey::survey(area, &|p| block_at(bot, p)))
}
