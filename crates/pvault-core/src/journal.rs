// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

use crate::account::{Account, AccountKey};

pub const SNAPSHOT_FORMAT: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub seq: u64,
    pub time: u64,
    pub sender: String,
    pub op: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    pub effects: Vec<Effect>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Effect {
    Set {
        account: AccountKey,
        balance: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner: Option<String>,
    },
    Remove {
        account: AccountKey,
    },
}

impl Effect {
    #[must_use]
    pub const fn account(&self) -> &AccountKey {
        match self {
            Self::Set { account, .. } | Self::Remove { account } => account,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub format: u32,
    pub seq: u64,
    pub accounts: std::collections::BTreeMap<AccountKey, Account>,
}

impl Snapshot {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let snapshot: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if snapshot.format > SNAPSHOT_FORMAT {
            return Err(format!(
                "snapshot format {} is newer than this plugin understands ({SNAPSHOT_FORMAT})",
                snapshot.format
            ));
        }
        Ok(snapshot)
    }

    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }
}

#[must_use]
pub fn parse_log(contents: &str) -> (Vec<LogEntry>, Vec<String>) {
    let mut entries = Vec::new();
    let mut problems = Vec::new();

    for (number, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<LogEntry>(line) {
            Ok(entry) => entries.push(entry),
            Err(error) => problems.push(format!("log line {}: {error}", number + 1)),
        }
    }

    (entries, problems)
}

pub fn encode_log_entry(entry: &LogEntry) -> Result<String, String> {
    serde_json::to_string(entry).map_err(|e| e.to_string())
}
