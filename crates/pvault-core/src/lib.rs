// SPDX-License-Identifier: GPL-3.0-or-later

pub mod account;
pub mod config;
pub mod journal;
pub mod ledger;
pub mod money;

pub use account::{Account, AccountKey};
pub use config::Config;
pub use journal::{Effect, LogEntry, Snapshot, encode_log_entry, parse_log};
pub use ledger::{ADMIN_SENDER, Economy, Outcome};
pub use money::{format_amount, parse_amount};

#[must_use]
pub fn recover(config: Config, snapshot: Option<Snapshot>, log: &str) -> (Economy, Vec<String>) {
    let mut economy = match snapshot {
        Some(snapshot) => Economy::from_snapshot(config, snapshot),
        None => Economy::new(config),
    };

    let (entries, problems) = parse_log(log);
    for entry in entries {
        if entry.seq > economy.seq() {
            economy.apply(&entry);
        }
    }

    (economy, problems)
}

#[cfg(test)]
mod tests;
