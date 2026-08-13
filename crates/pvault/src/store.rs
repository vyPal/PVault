// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use pvault_core::{Config, Economy, LogEntry, Snapshot, encode_log_entry};
use tracing::{info, warn};

const CONFIG_FILE: &str = "config.json";
const SNAPSHOT_FILE: &str = "economy.snapshot.json";
const LOG_FILE: &str = "economy.log.jsonl";

pub struct Store {
    snapshot_path: PathBuf,
    log_path: PathBuf,
    log: File,
    entries_since_snapshot: u64,
}

impl Store {
    pub fn open(folder: &Path) -> Result<(Self, Economy), String> {
        fs::create_dir_all(folder)
            .map_err(|e| format!("cannot create {}: {e}", folder.display()))?;

        let config = load_config(&folder.join(CONFIG_FILE));
        let snapshot_path = folder.join(SNAPSHOT_FILE);
        let log_path = folder.join(LOG_FILE);

        let snapshot = match fs::read_to_string(&snapshot_path) {
            Ok(contents) => match Snapshot::from_json(&contents) {
                Ok(snapshot) => Some(snapshot),
                Err(error) => {
                    warn!("ignoring unreadable snapshot: {error}");
                    None
                }
            },
            Err(_) => None,
        };

        let log = fs::read_to_string(&log_path).unwrap_or_default();
        let (economy, problems) = pvault_core::recover(config, snapshot, &log);
        for problem in &problems {
            warn!("{problem}");
        }

        let entries_since_snapshot =
            log.lines().filter(|line| !line.trim().is_empty()).count() as u64;
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|e| format!("cannot open {}: {e}", log_path.display()))?;

        info!(
            accounts = economy.accounts().len(),
            transactions = economy.seq(),
            "ledger loaded"
        );

        Ok((
            Self {
                snapshot_path,
                log_path,
                log,
                entries_since_snapshot,
            },
            economy,
        ))
    }

    pub fn append(&mut self, entry: &LogEntry) -> Result<(), String> {
        let line = encode_log_entry(entry)?;
        self.log
            .write_all(format!("{line}\n").as_bytes())
            .and_then(|()| self.log.flush())
            .map_err(|e| format!("cannot write to the transaction log: {e}"))?;
        let _ = self.log.sync_data();
        self.entries_since_snapshot += 1;
        Ok(())
    }

    #[must_use]
    pub const fn needs_compaction(&self, threshold: u64) -> bool {
        self.entries_since_snapshot >= threshold
    }

    pub fn compact(&mut self, economy: &Economy) -> Result<(), String> {
        if self.entries_since_snapshot == 0 {
            return Ok(());
        }

        let json = economy.snapshot().to_json()?;
        let temporary = self.snapshot_path.with_extension("json.tmp");
        fs::write(&temporary, json)
            .map_err(|e| format!("cannot write {}: {e}", temporary.display()))?;
        fs::rename(&temporary, &self.snapshot_path)
            .map_err(|e| format!("cannot replace the snapshot: {e}"))?;

        self.log = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.log_path)
            .map_err(|e| format!("cannot truncate {}: {e}", self.log_path.display()))?;
        self.entries_since_snapshot = 0;
        Ok(())
    }
}

fn load_config(path: &Path) -> Config {
    let mut config = match fs::read_to_string(path) {
        Ok(contents) => Config::from_json(&contents).unwrap_or_else(|error| {
            warn!("config is unreadable ({error}), falling back to defaults");
            Config::default()
        }),
        Err(_) => Config::default(),
    };
    config.sanitize();

    if let Ok(json) = config.to_json()
        && fs::read_to_string(path).ok().as_deref() != Some(json.as_str())
        && let Err(error) = fs::write(path, json)
    {
        warn!("cannot write {}: {error}", path.display());
    }

    config
}
