// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use pvault_core::{Economy, format_amount};
use pvault_proto::{ErrorCode, Request, Response};
use tracing::{error, warn};

use crate::store::Store;

thread_local! {
    static SERVICE: RefCell<Option<Service>> = const { RefCell::new(None) };
}

// TODO: Replace with something else once wasip3 drops
pub struct Service {
    economy: Economy,
    store: Store,
}

pub fn init(folder: &str) -> Result<(), String> {
    let (store, economy) = Store::open(Path::new(folder))?;
    SERVICE.with_borrow_mut(|service| *service = Some(Service { economy, store }));
    Ok(())
}

pub fn with<R>(f: impl FnOnce(&mut Service) -> R) -> Option<R> {
    SERVICE.with_borrow_mut(|service| service.as_mut().map(f))
}

pub fn shutdown() {
    SERVICE.with_borrow_mut(|service| {
        if let Some(service) = service.as_mut() {
            service.flush();
        }
        *service = None;
    });
}

impl Service {
    pub fn execute(&mut self, sender: &str, request: &Request) -> Response {
        let outcome = self.economy.plan(sender, now_millis(), request);

        if let Some(entry) = &outcome.entry {
            if let Err(error) = self.store.append(entry) {
                error!("dropping a transaction from {sender}: {error}");
                return Response::error(
                    ErrorCode::StorageError,
                    "the transaction could not be written to disk, so it was not applied",
                );
            }
            self.economy.apply(entry);

            if self
                .store
                .needs_compaction(self.economy.config().log_compaction_threshold)
            {
                self.flush();
            }
        }

        outcome.response
    }

    pub fn flush(&mut self) {
        if let Err(error) = self.store.compact(&self.economy) {
            warn!("could not save the ledger: {error}");
        }
    }

    #[must_use]
    pub const fn economy(&self) -> &Economy {
        &self.economy
    }

    #[must_use]
    pub fn format(&self, amount: i64) -> String {
        let config = self.economy.config();
        format!(
            "{}{}",
            config.currency_symbol,
            format_amount(amount, config.fraction_digits)
        )
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(0))
}
