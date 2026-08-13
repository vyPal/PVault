// SPDX-License-Identifier: 0BSD

//! A minimal PVault consumer: it keeps a till, charges a player on join, and reports
//! what happened. Useful as a copy-paste starting point and as an end-to-end check
//! that IPC routing works on a real server.

use pumpkin_plugin_api::{
    Context, Plugin, PluginMetadata, Result, Server,
    events::{EventData, EventHandler, EventPriority, PlayerJoinEvent},
    register_plugin,
};
use pvault_ipc::{Economy, account};
use tracing::{info, warn};

const NAMESPACE: &str = "ipc-consumer";
const ENTRY_FEE: i64 = 25;

struct Consumer;

impl Plugin for Consumer {
    fn new() -> Self {
        Consumer
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "ipc-consumer".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            authors: vec!["vyPal".into()],
            description: env!("CARGO_PKG_DESCRIPTION").into(),
            dependencies: vec![pvault_ipc::PLUGIN_NAME.into()],
            permissions: vec![],
        }
    }

    fn on_load(&mut self, context: Context) -> Result<()> {
        let currency = Economy::currency().map_err(|e| e.to_string())?;
        info!(
            "PVault is running with {} ({} decimal places)",
            currency.plural, currency.fraction_digits
        );

        let till = account::named(NAMESPACE, "till");
        if !Economy::exists(&till).map_err(|e| e.to_string())? {
            Economy::create_account(&till, Some(0)).map_err(|e| e.to_string())?;
            info!("opened a till");
        }

        context.register_event_handler(JoinHandler, EventPriority::Normal, false)?;
        Ok(())
    }
}

struct JoinHandler;

impl EventHandler<PlayerJoinEvent> for JoinHandler {
    fn handle(
        &self,
        _server: Server,
        event: EventData<PlayerJoinEvent>,
    ) -> EventData<PlayerJoinEvent> {
        let player = account::player(&event.player.get_id());
        let till = account::named(NAMESPACE, "till");

        match Economy::transfer(&player, &till, ENTRY_FEE, "entry fee") {
            Ok(result) => info!(
                "charged {} an entry fee, {} left",
                event.player.get_name(),
                result.from.map_or(0, |balance| balance.amount)
            ),
            Err(error) if error.is_insufficient_funds() => {
                info!("{} could not afford the entry fee", event.player.get_name());
            }
            Err(error) => warn!("could not charge the entry fee: {error}"),
        }

        event
    }
}

register_plugin!(Consumer);
