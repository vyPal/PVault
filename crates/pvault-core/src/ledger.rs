// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeMap;

use pvault_proto::{
    AccountId, AccountStatus, Balance, CurrencyInfo, Error, ErrorCode, HasBalanceResult, Request,
    Response, SPEC_VERSION, TransferResult, request, response,
};

use crate::account::{Account, AccountKey, InvalidAccount};
use crate::config::Config;
use crate::journal::{Effect, LogEntry, SNAPSHOT_FORMAT, Snapshot};

pub const ADMIN_SENDER: &str = pvault_proto::PLUGIN_NAME;

pub struct Outcome {
    pub response: Response,
    pub entry: Option<LogEntry>,
}

pub struct Economy {
    config: Config,
    accounts: BTreeMap<AccountKey, Account>,
    seq: u64,
}

impl Economy {
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self {
            config,
            accounts: BTreeMap::new(),
            seq: 0,
        }
    }

    #[must_use]
    pub fn from_snapshot(config: Config, snapshot: Snapshot) -> Self {
        Self {
            config,
            accounts: snapshot.accounts,
            seq: snapshot.seq,
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            format: SNAPSHOT_FORMAT,
            seq: self.seq,
            accounts: self.accounts.clone(),
        }
    }

    #[must_use]
    pub const fn config(&self) -> &Config {
        &self.config
    }

    #[must_use]
    pub const fn seq(&self) -> u64 {
        self.seq
    }

    #[must_use]
    pub const fn accounts(&self) -> &BTreeMap<AccountKey, Account> {
        &self.accounts
    }

    #[must_use]
    pub fn exists(&self, account: &AccountKey) -> bool {
        self.accounts.contains_key(account)
    }

    #[must_use]
    pub fn balance(&self, account: &AccountKey) -> i64 {
        self.accounts.get(account).map_or_else(
            || {
                if account.is_player() {
                    self.config.starting_balance
                } else {
                    0
                }
            },
            |a| a.balance,
        )
    }

    pub fn apply(&mut self, entry: &LogEntry) {
        for effect in &entry.effects {
            match effect {
                Effect::Set {
                    account,
                    balance,
                    owner,
                } => {
                    self.accounts
                        .entry(account.clone())
                        .and_modify(|existing| existing.balance = *balance)
                        .or_insert_with(|| Account {
                            balance: *balance,
                            owner: owner.clone(),
                        });
                }
                Effect::Remove { account } => {
                    self.accounts.remove(account);
                }
            }
        }
        self.seq = self.seq.max(entry.seq);
    }

    #[must_use]
    pub fn plan(&self, sender: &str, now: u64, request: &Request) -> Outcome {
        let Some(version) = request.version else {
            return read_only(Response::error(
                ErrorCode::Malformed,
                "request is missing a version",
            ));
        };
        if !SPEC_VERSION.is_compatible_with(&version) {
            return read_only(Response::error(
                ErrorCode::VersionMismatch,
                format!("PVault speaks {SPEC_VERSION}, this message is for {version}"),
            ));
        }

        let Some(body) = &request.body else {
            return read_only(Response::error(
                ErrorCode::UnknownRequest,
                "request has no body, or one this version of PVault does not know",
            ));
        };

        match self.handle(sender, now, body) {
            Ok(outcome) => outcome,
            Err(response) => read_only(response),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn handle(&self, sender: &str, now: u64, body: &request::Body) -> Result<Outcome, Response> {
        match body {
            request::Body::GetCurrency(_) => Ok(read_only(Response::new(
                response::Body::Currency(CurrencyInfo {
                    name: self.config.currency_name.clone(),
                    plural: self.config.currency_plural.clone(),
                    symbol: self.config.currency_symbol.clone(),
                    fraction_digits: self.config.fraction_digits,
                    starting_balance: self.config.starting_balance,
                }),
            ))),

            request::Body::GetBalance(req) => {
                let account = self.resolve_existing(req.account.as_ref())?;
                let balance = self.balance(&account);
                Ok(read_only(balance_response(&account, balance, balance)))
            }

            request::Body::HasBalance(req) => {
                let account = self.resolve_existing(req.account.as_ref())?;
                check_amount(req.amount)?;
                let balance = self.balance(&account);
                Ok(read_only(Response::new(response::Body::HasBalance(
                    HasBalanceResult {
                        sufficient: balance >= req.amount,
                        balance,
                    },
                ))))
            }

            request::Body::AccountExists(req) => {
                let account = resolve(req.account.as_ref())?;
                Ok(read_only(Response::new(response::Body::Account(
                    AccountStatus {
                        exists: self.exists(&account),
                        balance: self.balance(&account),
                    },
                ))))
            }

            request::Body::Deposit(req) => {
                let account = self.resolve_existing(req.account.as_ref())?;
                check_amount(req.amount)?;
                let previous = self.balance(&account);
                let balance = previous.checked_add(req.amount).ok_or_else(overflow)?;
                Ok(self.mutation(
                    sender,
                    now,
                    "deposit",
                    &req.reason,
                    vec![self.set_effect(&account, balance)],
                    balance_response(&account, balance, previous),
                ))
            }

            request::Body::Withdraw(req) => {
                let account = self.resolve_existing(req.account.as_ref())?;
                check_amount(req.amount)?;
                let previous = self.balance(&account);
                let balance = previous
                    .checked_sub(req.amount)
                    .filter(|balance| *balance >= 0)
                    .ok_or_else(|| insufficient_funds(previous))?;
                Ok(self.mutation(
                    sender,
                    now,
                    "withdraw",
                    &req.reason,
                    vec![self.set_effect(&account, balance)],
                    balance_response(&account, balance, previous),
                ))
            }

            request::Body::Transfer(req) => {
                let from = self.resolve_existing(req.from.as_ref())?;
                let to = self.resolve_existing(req.to.as_ref())?;
                check_amount(req.amount)?;
                if from == to {
                    return Err(Response::error(
                        ErrorCode::InvalidAccount,
                        "an account cannot pay itself",
                    ));
                }

                let from_before = self.balance(&from);
                let to_before = self.balance(&to);
                let from_after = from_before
                    .checked_sub(req.amount)
                    .filter(|balance| *balance >= 0)
                    .ok_or_else(|| insufficient_funds(from_before))?;
                let to_after = to_before.checked_add(req.amount).ok_or_else(overflow)?;

                Ok(self.mutation(
                    sender,
                    now,
                    "transfer",
                    &req.reason,
                    vec![
                        self.set_effect(&from, from_after),
                        self.set_effect(&to, to_after),
                    ],
                    Response::new(response::Body::Transfer(TransferResult {
                        from: Some(balance(&from, from_after, from_before)),
                        to: Some(balance(&to, to_after, to_before)),
                    })),
                ))
            }

            request::Body::SetBalance(req) => {
                let account = self.resolve_existing(req.account.as_ref())?;
                check_amount(req.amount)?;
                self.check_owner(sender, &account)?;
                let previous = self.balance(&account);
                Ok(self.mutation(
                    sender,
                    now,
                    "set",
                    &req.reason,
                    vec![self.set_effect(&account, req.amount)],
                    balance_response(&account, req.amount, previous),
                ))
            }

            request::Body::CreateAccount(req) => {
                let account = resolve(req.account.as_ref())?;
                if self.exists(&account) {
                    return Err(Response::error(
                        ErrorCode::AccountExists,
                        format!("account {account} already exists"),
                    ));
                }
                let balance = req.initial_balance.unwrap_or_else(|| {
                    if account.is_player() {
                        self.config.starting_balance
                    } else {
                        0
                    }
                });
                check_amount(balance)?;

                let owner = (!account.is_player()).then(|| sender.to_owned());
                Ok(self.mutation(
                    sender,
                    now,
                    "create",
                    "",
                    vec![Effect::Set {
                        account: account.clone(),
                        balance,
                        owner,
                    }],
                    Response::new(response::Body::Account(AccountStatus {
                        exists: true,
                        balance,
                    })),
                ))
            }

            request::Body::DeleteAccount(req) => {
                let account = resolve(req.account.as_ref())?;
                if !self.exists(&account) {
                    return Err(not_found(&account));
                }
                self.check_owner(sender, &account)?;
                Ok(self.mutation(
                    sender,
                    now,
                    "delete",
                    "",
                    vec![Effect::Remove {
                        account: account.clone(),
                    }],
                    Response::new(response::Body::Account(AccountStatus {
                        exists: false,
                        balance: 0,
                    })),
                ))
            }
        }
    }

    fn mutation(
        &self,
        sender: &str,
        now: u64,
        op: &str,
        reason: &str,
        effects: Vec<Effect>,
        response: Response,
    ) -> Outcome {
        Outcome {
            response,
            entry: Some(LogEntry {
                seq: self.seq + 1,
                time: now,
                sender: sender.to_owned(),
                op: op.to_owned(),
                reason: reason.to_owned(),
                effects,
            }),
        }
    }

    fn set_effect(&self, account: &AccountKey, balance: i64) -> Effect {
        Effect::Set {
            account: account.clone(),
            balance,
            owner: self
                .accounts
                .get(account)
                .and_then(|existing| existing.owner.clone()),
        }
    }

    fn resolve_existing(&self, id: Option<&AccountId>) -> Result<AccountKey, Response> {
        let account = resolve(id)?;
        if account.is_player() || self.exists(&account) {
            Ok(account)
        } else {
            Err(not_found(&account))
        }
    }

    fn check_owner(&self, sender: &str, account: &AccountKey) -> Result<(), Response> {
        if sender == ADMIN_SENDER {
            return Ok(());
        }
        match self.accounts.get(account).and_then(|a| a.owner.as_deref()) {
            Some(owner) if owner != sender => Err(Response::error(
                ErrorCode::PermissionDenied,
                format!("account {account} belongs to {owner}"),
            )),
            _ => Ok(()),
        }
    }
}

fn resolve(id: Option<&AccountId>) -> Result<AccountKey, Response> {
    AccountKey::from_proto(id).map_err(|error| {
        let code = match error {
            InvalidAccount::Missing => ErrorCode::Malformed,
            InvalidAccount::BadUuid | InvalidAccount::BadName => ErrorCode::InvalidAccount,
        };
        Response::error(code, error.to_string())
    })
}

fn check_amount(amount: i64) -> Result<(), Response> {
    if amount < 0 {
        return Err(Response::error(
            ErrorCode::InvalidAmount,
            "amounts cannot be negative",
        ));
    }
    Ok(())
}

fn not_found(account: &AccountKey) -> Response {
    Response::error(
        ErrorCode::AccountNotFound,
        format!("no account named {account}"),
    )
}

fn overflow() -> Response {
    Response::error(
        ErrorCode::Overflow,
        "that would push the balance past the maximum",
    )
}

fn insufficient_funds(balance: i64) -> Response {
    let mut error = Error::new(ErrorCode::InsufficientFunds, "not enough funds");
    error.balance = balance;
    Response::new(response::Body::Error(error))
}

fn balance(account: &AccountKey, amount: i64, previous: i64) -> Balance {
    Balance {
        account: Some(account.to_proto()),
        amount,
        previous,
    }
}

fn balance_response(account: &AccountKey, amount: i64, previous: i64) -> Response {
    Response::new(response::Body::Balance(balance(account, amount, previous)))
}

const fn read_only(response: Response) -> Outcome {
    Outcome {
        response,
        entry: None,
    }
}
