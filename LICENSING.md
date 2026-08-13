# Licensing

PVault is split in two, along the line between *the plugin* and *the thing you call it with*.

| Path | License | |
|---|---|---|
| `crates/pvault` | GPL-3.0-or-later | the plugin itself |
| `crates/pvault-core` | GPL-3.0-or-later | the ledger |
| `crates/pvault-proto` | 0BSD | generated message types |
| `crates/pvault-ipc` | 0BSD | client for other plugins |
| `proto/`, `docs/spec/` | 0BSD | the protocol |
| `examples/`, `tools/` | 0BSD | examples and build tooling |

Full texts: [`LICENSE`](LICENSE) (GPL-3.0) and [`LICENSE-0BSD`](LICENSE-0BSD).

## If you are writing a plugin that uses PVault

PVault asks nothing of you. The client crates, the `.proto`, and the spec are
[0BSD](LICENSE-0BSD) — public-domain-equivalent, with no attribution clause. Use them
however you like, in whatever you like, and you owe nobody a notice, a credit, or a line
of source.

That is a deliberate choice: depending on the server's economy should never be a
licensing decision.

## If you are forking PVault

The plugin and the ledger are GPL-3.0-or-later. Distribute a modified PVault, or a plugin
built out of its code, and the source has to come with it.

## One thing that is not ours to give

[Pumpkin](https://github.com/Pumpkin-MC/Pumpkin) is GPL-3.0 and grants no linking
exception, and `pumpkin-plugin-api` inherits that. Every Pumpkin plugin links it, so every
Pumpkin plugin is arguably already a combined work with GPL-3.0 code — whether or not it
ever touches PVault.

So while PVault imposes nothing on you, we cannot promise your plugin can stay closed;
that question is between you and Pumpkin's license, and it is unsettled. There is a
reasonable argument that plugins are separate programs — they are isolated wasm components
talking to the host over WIT component-model imports, which resembles a syscall boundary
far more than it resembles linking — but the API crate compiling into your binary muddies
it. If you want certainty, the fix is upstream: a §7 plugin linking exception on Pumpkin
would settle it for the whole ecosystem.
