// SPDX-License-Identifier: GPL-3.0-or-later

use pumpkin_plugin_api::{
    Context, Plugin, PluginMetadata, Result, permissions, register_plugin, scheduler::SchedulerExt,
};
use pvault_proto::{Message, Request, Response};
use tracing::{error, info};

mod commands;
mod service;
mod store;

struct PVault;

impl Plugin for PVault {
    fn new() -> Self {
        PVault
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: pvault_proto::PLUGIN_NAME.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            authors: env!("CARGO_PKG_AUTHORS")
                .split(',')
                .map(str::to_string)
                .collect(),
            description: env!("CARGO_PKG_DESCRIPTION").into(),
            dependencies: vec![],
            permissions: vec![
                permissions::FS_READ_DATA.into(),
                permissions::FS_WRITE_DATA.into(),
            ],
        }
    }

    fn on_load(&mut self, context: Context) -> Result<()> {
        service::init(&context.get_data_folder())?;
        commands::register(&context)?;

        let interval =
            service::with(|service| service.economy().config().autosave_ticks).unwrap_or(6000);
        context.schedule_repeating_task(interval, interval, |_| {
            service::with(service::Service::flush);
        });

        info!(
            "PVault ready, speaking economy spec {}",
            pvault_proto::SPEC_VERSION
        );
        Ok(())
    }

    fn on_unload(&mut self, _context: Context) -> Result<()> {
        service::shutdown();
        Ok(())
    }

    fn handle_ipc_message(&mut self, sender: String, message: Vec<u8>) -> Result<Vec<u8>, String> {
        let request = Request::decode(message.as_slice()).map_err(|error| {
            error!("{sender} sent something that is not a PVault request: {error}");
            format!("expected a pvault.economy.v1.Request: {error}")
        })?;

        let response: Response = service::with(|service| service.execute(&sender, &request))
            .ok_or_else(|| "PVault is not loaded".to_string())?;

        Ok(response.encode_to_vec())
    }
}

register_plugin!(PVault);
