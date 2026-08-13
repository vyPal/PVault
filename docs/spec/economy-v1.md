# PVault economy API spec 1.x

How other Pumpkin plugins talk to PVault's economy. The message definitions live in [`proto/pvault/economy/v1/economy.proto`](../../proto/pvault/economy/v1/economy.proto); this document explains what the messages mean.

## Transport

PVault registers itself under the plugin name **`PVault`**. That string is the IPC recipient id:

```rust
let reply = pumpkin_plugin_api::ipc::send_ipc_message("PVault", &request_bytes);
```

The payload is an encoded `pvault.economy.v1.Request`; the reply is an encoded `pvault.economy.v1.Response`. Calls are synchronous (since Pumpkin uses wasip2 at the moment), the host runs PVault's handler and hands you the bytes back.

Three failures happen below the message layer:

| What you get | Means |
|---|---|
| `Err(())` from the host | PVault isn't installed, isn't loaded, or you addressed yourself |
| `Ok(Err(string))` | PVault could not decode your bytes as a `Request` |
| `Ok(Ok(bytes))` | A real `Response` which may still carry an `Error` body |

Everything PVault can explain comes back as a `Response` with an `Error` body, so you always learn which version answered you.

## Versioning

The spec version is the plugin version, semver. `Request.version` and `Response.version` are mandatory on both sides.

- **Major must match.** A request whose major differs is rejected with `VERSION_MISMATCH` and changes nothing.
- **Minor and patch are advisory.** If you ask for a newer minor than PVault implements, you are served anyway and the response tells you the real version — unknown fields you sent are ignored by protobuf, so you may silently not get behaviour that doesn't exist yet. Compare `Response.version` against what you need if that matters to you.
- A request with no `version` at all is `MALFORMED`.

Breaking changes, anything that gets a major bump:

- removing or renumbering a field, message, or `ErrorCode`
- changing the meaning or units of an existing field
- making a previously accepted request fail

Not breaking, so a minor bump:

- new request or response bodies, new optional fields, new `ErrorCode` values
- accepting requests that used to be rejected

If you receive an `ErrorCode` you don't recognise, treat it as `UNSPECIFIED` and show the message.

## Money

One currency, amounts as `int64` **minor units** - whole numbers in the smallest unit the server's currency has. `GetCurrency` reports `fraction_digits` (2 means the minor unit is a hundredth), plus `name`, `plural`, `symbol`, and `starting_balance`

Amounts on `deposit`, `withdraw`, `transfer`, `set_balance` and `create_account` must be `>= 0`. Negative amounts are `INVALID_AMOUNT`. Balances never go below zero in this version.

## Accounts

`AccountId` is either:

- **`player`** - a 16-byte big-endian UUID. Any other length is `INVALID_ACCOUNT`. Player accounts are implicit: an unknown player reads as `starting_balance` and is materialized the first time something changes their balance.
- **`named`** - `"namespace:key"`, at most 128 characters of `[a-zA-Z0-9_.-]` on each side of a single colon. Use your plugin name as the namespace. Named accounts do **not** spring into existence: use `CreateAccount` first, or you get `ACCOUNT_NOT_FOUND`. That way a typo in the key can't silently create an account.

The plugin that creates a named account owns it. Anyone may `Deposit` to or `Withdraw` from it, but `SetBalance` and `DeleteAccount` are owner-only and answer `PERMISSION_DENIED` to anyone else. PVault's own commands bypass ownership.

## Requests

| Request | Response body | Notes |
|---|---|---|
| `GetCurrency` | `CurrencyInfo` | Cache it; it only changes when the server owner edits the config |
| `GetBalance` | `Balance` | `previous == amount` |
| `HasBalance` | `HasBalanceResult` | `sufficient` is `balance >= amount`; the balance comes along |
| `Deposit` | `Balance` | `OVERFLOW` if the balance would pass `int64` |
| `Withdraw` | `Balance` | `INSUFFICIENT_FUNDS` leaves the balance untouched |
| `Transfer` | `TransferResult` | All-or-nothing. Paying yourself is `INVALID_ACCOUNT` |
| `SetBalance` | `Balance` | Owner-only on named accounts |
| `CreateAccount` | `AccountStatus` | `initial_balance` defaults to `starting_balance` for players, 0 for named. `ACCOUNT_EXISTS` if it's already there |
| `AccountExists` | `AccountStatus` | Never errors on unknown accounts - `exists` is just false |
| `DeleteAccount` | `AccountStatus` | Owner-only on named accounts. Deleting a player account resets them to `starting_balance` |

`Balance` carries `previous` on mutations, so you can show a delta without a second round-trip.

`reason` on the mutating requests is free text recorded in PVault's transaction log. It's for
server owners reading the audit trail, used to write something useful ("bought 3 diamonds").

## Errors

| Code | When |
|---|---|
| `MALFORMED` | Decodable protobuf, but a required piece is missing (no version, no account) |
| `VERSION_MISMATCH` | Different major version |
| `UNKNOWN_REQUEST` | No body, or a body this version doesn't implement |
| `INVALID_ACCOUNT` | Malformed UUID or account name, or an account paying itself |
| `ACCOUNT_NOT_FOUND` | A named account that was never created |
| `ACCOUNT_EXISTS` | `CreateAccount` on an account that exists |
| `INSUFFICIENT_FUNDS` | The balance can't cover it. `Error.balance` holds the current balance |
| `INVALID_AMOUNT` | Negative amount |
| `PERMISSION_DENIED` | Touching another plugin's named account |
| `OVERFLOW` | The result wouldn't fit in `int64` |
| `STORAGE_ERROR` | The transaction couldn't be journalled, so it was **not** applied |

`Error.balance` is only meaningful for `INSUFFICIENT_FUNDS`; it's 0 everywhere else.

## Guarantees

- A request either fully applies or changes nothing. There is no partial transfer.
- A mutation is written to the transaction log and flushed **before** it is applied in memory, so a crash can't acknowledge a change that then vanishes. If the write fails you get `STORAGE_ERROR` and nothing changed.
- Requests are handled one at a time, in order.
- There is no idempotency key in 1.x. Sending the same deposit twice deposits twice. Since the call is synchronous and in-process, there's no delivery retry to worry about, but don't retry a mutation after a timeout without checking the balance first.

## Rust client

Skip the encoding entirely with the [`pvault-ipc`](../../crates/pvault-ipc) crate:

```rust
use pvault_ipc::{Economy, account};

let wallet = account::player(&player.get_id());
let till = account::named("myshop", "till");

match Economy::transfer(&wallet, &till, price, "bought a pickaxe") {
    Ok(_) => player.send_system_message(TextComponent::text("Sold!"), false),
    Err(error) if error.is_insufficient_funds() => { /* too poor */ }
    Err(error) => warn!("economy call failed: {error}"),
}
```

For other languages or your own bindings, generate from the `.proto`, that file is the contract,
not this crate.
