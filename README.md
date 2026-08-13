# PVault

A central economy for [Pumpkin](https://github.com/Pumpkin-MC/Pumpkin), in the spirit of Bukkit's Vault: one plugin owns the balances, everyone else asks it over plugin IPC instead of shipping their own wallet.

Right now only the economy API is implemented. Chat, permissions and placeholder APIs are on the list.

## For server owners

Drop `pvault.wasm` into your server's `plugins/` folder. On first start PVault writes a `config.json` next to its data:

```json
{
  "currency_name": "coin",
  "currency_plural": "coins",
  "currency_symbol": "$",
  "fraction_digits": 2,
  "starting_balance": 0,
  "autosave_ticks": 6000,
  "log_compaction_threshold": 5000
}
```

Balances live in `economy.snapshot.json` with a `economy.log.jsonl` transaction log beside it. Both are plain text so that you can read them and the log tells you which plugin asked for every change. The log get's truncated automatically to not use up too much space.

Commands: `/balance [player]` (aliases `/bal`, `/money`), `/pay <player> <amount>`, and `/eco give|take|set <player> <amount>` / `/eco reset <player>`. Permissions are `PVault:command.balance`, `PVault:command.balance.other`, `PVault:command.pay` and `PVault:command.eco`.

> [!NOTE]
> `/pay` only reaches players who are online.

## For plugin authors

PVault listens on the IPC name `PVault` and speaks protobuf. The contract is [`proto/pvault/economy/v1/economy.proto`](proto/pvault/economy/v1/economy.proto), explained in [`docs/spec/economy-v1.md`](docs/spec/economy-v1.md). It's versioned with the plugin: same major version means you should be compatible.

In Rust, use the client crate and skip the wire format:

```toml
[dependencies]
pvault-ipc = { git = "https://github.com/vyPal/PVault" }
```

```rust
use pvault_ipc::{Economy, account};

let wallet = account::player(&player.get_id());
if Economy::has(&wallet, price)? {
    Economy::withdraw(&wallet, price, "bought a diamond pickaxe")?;
}
```

[`examples/ipc-consumer`](examples/ipc-consumer) is a complete plugin doing this.

## Building

```sh
cargo build --release              # the plugin, for wasm32-wasip2
cargo test --target x86_64-unknown-linux-gnu -p pvault-core -p pvault-proto
```

`.cargo/config.toml` targets wasm, so the pure crates need an explicit host target to run their tests. After editing the `.proto`, regenerate the checked-in bindings:

```sh
cargo run -p proto-gen --target x86_64-unknown-linux-gnu
```

That uses [protox](https://github.com/andrewhickman/protox), so you don't need `protoc` installed.

## Layout

| crate | what it is |
|---|---|
| `crates/pvault` | the plugin: IPC entry point, storage, commands |
| `crates/pvault-core` | the ledger and request dispatch, unit-tested |
| `crates/pvault-proto` | generated message types, the published contract |
| `crates/pvault-ipc` | typed client for other plugins |

## License

The plugin (`crates/pvault`, `crates/pvault-core`) is GPL-3.0-or-later. Fork it and your version has to stay open.

Everything you need in order to *talk* to it — `pvault-ipc`, `pvault-proto`, the `.proto`, the spec, the example — is [0BSD](LICENSE-0BSD): public domain equivalent, no attribution required. PVault asks nothing of plugins that use it.

Note that Pumpkin itself is GPL-3.0 with no linking exception, and every plugin links `pumpkin-plugin-api`, so that question is between you and Pumpkin regardless of PVault. [`LICENSING.md`](LICENSING.md) has the details.
