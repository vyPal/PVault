// SPDX-License-Identifier: GPL-3.0-or-later

use pumpkin_plugin_api::{
    Context, Result, Server,
    command::{Command, CommandError, CommandNode, CommandSender, ConsumedArgs},
    command_wit::{Arg, ArgumentType, StringType},
    commands::CommandHandler,
    common::NamedColor,
    permission::{Permission, PermissionDefault, PermissionLevel},
    player::Player,
    text::TextComponent,
    uuid::Uuid,
};
use pvault_core::{ADMIN_SENDER, parse_amount};
use pvault_proto::{
    AccountId, Deposit, Request, Response, SetBalance, Transfer, Withdraw, request, response,
};

use crate::service;

const TARGET: &str = "player";
const AMOUNT: &str = "amount";

const BALANCE_PERMISSION: &str = "PVault:command.balance";
const BALANCE_OTHER_PERMISSION: &str = "PVault:command.balance.other";
const PAY_PERMISSION: &str = "PVault:command.pay";
const ECO_PERMISSION: &str = "PVault:command.eco";

pub fn register(context: &Context) -> Result<()> {
    for permission in [
        Permission {
            node: BALANCE_PERMISSION.into(),
            description: "Check your own balance".into(),
            default: PermissionDefault::Allow,
            children: Vec::new(),
        },
        Permission {
            node: BALANCE_OTHER_PERMISSION.into(),
            description: "Check someone else's balance".into(),
            default: PermissionDefault::Op(PermissionLevel::Two),
            children: Vec::new(),
        },
        Permission {
            node: PAY_PERMISSION.into(),
            description: "Pay another player".into(),
            default: PermissionDefault::Allow,
            children: Vec::new(),
        },
        Permission {
            node: ECO_PERMISSION.into(),
            description: "Change balances directly".into(),
            default: PermissionDefault::Op(PermissionLevel::Three),
            children: Vec::new(),
        },
    ] {
        context.register_permission(&permission)?;
    }

    let balance = Command::new(
        &[
            "balance".to_string(),
            "bal".to_string(),
            "money".to_string(),
        ],
        "Show a balance",
    );
    balance.then(CommandNode::argument(TARGET, &ArgumentType::Players).execute(BalanceCommand));
    let balance = balance.execute(BalanceCommand);
    context.register_command(balance, BALANCE_PERMISSION);

    let pay = Command::new(&["pay".to_string()], "Pay another player");
    let target = CommandNode::argument(TARGET, &ArgumentType::Players);
    target.then(amount_node(PayCommand));
    pay.then(target);
    context.register_command(pay, PAY_PERMISSION);

    let eco = Command::new(&["eco".to_string()], "Manage balances");
    for (verb, action) in [
        ("give", Action::Give),
        ("take", Action::Take),
        ("set", Action::Set),
    ] {
        let target = CommandNode::argument(TARGET, &ArgumentType::Players);
        target.then(amount_node(EcoCommand(action)));
        let literal = CommandNode::literal(verb);
        literal.then(target);
        eco.then(literal);
    }
    let reset = CommandNode::literal("reset");
    reset.then(
        CommandNode::argument(TARGET, &ArgumentType::Players).execute(EcoCommand(Action::Reset)),
    );
    eco.then(reset);
    context.register_command(eco, ECO_PERMISSION);

    Ok(())
}

fn amount_node<H: CommandHandler + 'static>(handler: H) -> CommandNode {
    CommandNode::argument(AMOUNT, &ArgumentType::String(StringType::SingleWord)).execute(handler)
}

struct BalanceCommand;

impl CommandHandler for BalanceCommand {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let targets = players(&args);

        if targets.is_empty() {
            let player = require_player(&sender)?;
            let balance = balance_of(&player)?;
            reply(&sender, &format!("You have {balance}."), NamedColor::Green);
            return Ok(1);
        }

        if !sender.has_permission(&server, BALANCE_OTHER_PERMISSION) {
            return Err(CommandError::PermissionDenied);
        }
        for target in &targets {
            let balance = balance_of(target)?;
            reply(
                &sender,
                &format!("{} has {balance}.", target.get_name()),
                NamedColor::Green,
            );
        }
        Ok(1)
    }
}

struct PayCommand;

impl CommandHandler for PayCommand {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let payer = require_player(&sender)?;
        let target = single_target(&args, &sender)?;
        if uuid_bytes(&target.get_id()) == uuid_bytes(&payer.get_id()) {
            return Err(failed("You cannot pay yourself."));
        }
        let amount = amount(&args, &sender)?;

        let response = execute(
            ADMIN_SENDER,
            request::Body::Transfer(Transfer {
                from: Some(AccountId::player(uuid_bytes(&payer.get_id()))),
                to: Some(AccountId::player(uuid_bytes(&target.get_id()))),
                amount,
                reason: format!("/pay from {}", payer.get_name()),
            }),
        )?;

        match response.body {
            Some(response::Body::Transfer(result)) => {
                let paid = format_amount(amount)?;
                reply(
                    &sender,
                    &format!("Paid {paid} to {}.", target.get_name()),
                    NamedColor::Green,
                );
                let left = format_amount(result.from.map_or(0, |balance| balance.amount))?;
                reply(&sender, &format!("You now have {left}."), NamedColor::Gray);

                let note = TextComponent::text(&format!("{} paid you {paid}.", sender.get_name()));
                note.color_named(NamedColor::Green);
                target.send_system_message(note, false);
                Ok(1)
            }
            _ => Err(failed(&describe(&response))),
        }
    }
}

#[derive(Clone, Copy)]
enum Action {
    Give,
    Take,
    Set,
    Reset,
}

struct EcoCommand(Action);

impl CommandHandler for EcoCommand {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let targets = players(&args);
        if targets.is_empty() {
            return Err(failed("Name at least one player."));
        }

        let amount = match self.0 {
            Action::Reset => starting_balance()?,
            _ => amount(&args, &sender)?,
        };

        for target in &targets {
            let account = AccountId::player(uuid_bytes(&target.get_id()));
            let reason = format!("/eco by {}", sender.get_name());
            let body = match self.0 {
                Action::Give => request::Body::Deposit(Deposit {
                    account: Some(account),
                    amount,
                    reason,
                }),
                Action::Take => request::Body::Withdraw(Withdraw {
                    account: Some(account),
                    amount,
                    reason,
                }),
                Action::Set | Action::Reset => request::Body::SetBalance(SetBalance {
                    account: Some(account),
                    amount,
                    reason,
                }),
            };

            let response = execute(ADMIN_SENDER, body)?;
            match response.body {
                Some(response::Body::Balance(balance)) => {
                    let now = format_amount(balance.amount)?;
                    reply(
                        &sender,
                        &format!("{} now has {now}.", target.get_name()),
                        NamedColor::Green,
                    );
                }
                _ => return Err(failed(&describe(&response))),
            }
        }
        Ok(1)
    }
}

fn execute(sender: &str, body: request::Body) -> Result<Response, CommandError> {
    service::with(|service| service.execute(sender, &Request::new(body))).ok_or_else(not_ready)
}

fn balance_of(player: &Player) -> Result<String, CommandError> {
    let account = AccountId::player(uuid_bytes(&player.get_id()));
    let response = execute(
        ADMIN_SENDER,
        request::Body::GetBalance(pvault_proto::GetBalance {
            account: Some(account),
        }),
    )?;

    match response.body {
        Some(response::Body::Balance(balance)) => format_amount(balance.amount),
        _ => Err(failed(&describe(&response))),
    }
}

fn format_amount(amount: i64) -> Result<String, CommandError> {
    service::with(|service| service.format(amount)).ok_or_else(not_ready)
}

fn starting_balance() -> Result<i64, CommandError> {
    service::with(|service| service.economy().config().starting_balance).ok_or_else(not_ready)
}

fn players(args: &ConsumedArgs) -> Vec<Player> {
    match args.get_value(TARGET) {
        Arg::Players(players) => players,
        _ => Vec::new(),
    }
}

fn single_target(args: &ConsumedArgs, sender: &CommandSender) -> Result<Player, CommandError> {
    let mut targets = players(args);
    if targets.len() != 1 {
        reply(sender, "Name exactly one player.", NamedColor::Red);
        return Err(CommandError::InvalidRequirement);
    }
    Ok(targets.remove(0))
}

fn amount(args: &ConsumedArgs, sender: &CommandSender) -> Result<i64, CommandError> {
    let Arg::Simple(text) = args.get_value(AMOUNT) else {
        return Err(failed("Give an amount."));
    };
    let digits = service::with(|service| service.economy().config().fraction_digits)
        .ok_or_else(not_ready)?;

    parse_amount(&text, digits).map_err(|error| {
        reply(sender, &format!("{error}."), NamedColor::Red);
        CommandError::InvalidRequirement
    })
}

fn require_player(sender: &CommandSender) -> Result<Player, CommandError> {
    sender
        .as_player()
        .ok_or_else(|| failed("Only a player can run that."))
}

fn uuid_bytes(uuid: &Uuid) -> [u8; 16] {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&uuid.high.to_be_bytes());
    bytes[8..].copy_from_slice(&uuid.low.to_be_bytes());
    bytes
}

fn describe(response: &Response) -> String {
    match &response.body {
        Some(response::Body::Error(error)) => error.message.clone(),
        _ => "PVault returned something unexpected.".to_string(),
    }
}

fn reply(sender: &CommandSender, message: &str, color: NamedColor) {
    let text = TextComponent::text(message);
    text.color_named(color);
    sender.send_message(text);
}

fn failed(message: &str) -> CommandError {
    CommandError::CommandFailed(TextComponent::text(message))
}

fn not_ready() -> CommandError {
    failed("PVault is still loading.")
}
