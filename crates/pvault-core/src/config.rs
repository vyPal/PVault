// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

use crate::money::MAX_FRACTION_DIGITS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub currency_name: String,
    pub currency_plural: String,
    pub currency_symbol: String,
    pub fraction_digits: u32,
    pub starting_balance: i64,
    pub autosave_ticks: u64,
    pub log_compaction_threshold: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            currency_name: "coin".into(),
            currency_plural: "coins".into(),
            currency_symbol: "$".into(),
            fraction_digits: 2,
            starting_balance: 0,
            autosave_ticks: 6000,
            log_compaction_threshold: 5000,
        }
    }
}

impl Config {
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }

    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }

    pub fn sanitize(&mut self) {
        self.fraction_digits = self.fraction_digits.min(MAX_FRACTION_DIGITS);
        self.starting_balance = self.starting_balance.max(0);
        self.autosave_ticks = self.autosave_ticks.max(20);
        self.log_compaction_threshold = self.log_compaction_threshold.max(100);
    }
}
