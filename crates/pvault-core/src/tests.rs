// SPDX-License-Identifier: GPL-3.0-or-later

use pvault_proto::{
    AccountId, Deposit, ErrorCode, GetBalance, Request, Response, SetBalance, Transfer, Version,
    Withdraw, request, response,
};

use crate::account::AccountKey;
use crate::config::Config;
use crate::ledger::{ADMIN_SENDER, Economy};
use crate::{Snapshot, encode_log_entry, recover};

const ALICE: [u8; 16] = [1; 16];
const BOB: [u8; 16] = [2; 16];

fn economy() -> Economy {
    Economy::new(Config {
        starting_balance: 100,
        ..Config::default()
    })
}

/// Runs a request the way the plugin does: plan, journal, apply.
fn send(economy: &mut Economy, sender: &str, body: request::Body) -> Response {
    let outcome = economy.plan(sender, 0, &Request::new(body));
    if let Some(entry) = &outcome.entry {
        encode_log_entry(entry).expect("entries must serialize");
        economy.apply(entry);
    }
    outcome.response
}

fn error_code(response: &Response) -> ErrorCode {
    match &response.body {
        Some(response::Body::Error(error)) => error.code(),
        other => panic!("expected an error, got {other:?}"),
    }
}

fn balance_of(response: &Response) -> i64 {
    match &response.body {
        Some(response::Body::Balance(balance)) => balance.amount,
        other => panic!("expected a balance, got {other:?}"),
    }
}

fn deposit(account: AccountId, amount: i64) -> request::Body {
    request::Body::Deposit(Deposit {
        account: Some(account),
        amount,
        reason: String::new(),
    })
}

fn withdraw(account: AccountId, amount: i64) -> request::Body {
    request::Body::Withdraw(Withdraw {
        account: Some(account),
        amount,
        reason: String::new(),
    })
}

fn get_balance(account: AccountId) -> request::Body {
    request::Body::GetBalance(GetBalance {
        account: Some(account),
    })
}

fn create(account: AccountId, initial: Option<i64>) -> request::Body {
    request::Body::CreateAccount(pvault_proto::CreateAccount {
        account: Some(account),
        initial_balance: initial,
    })
}

#[test]
fn unknown_players_report_the_starting_balance_without_being_created() {
    let mut economy = economy();
    let response = send(&mut economy, "shop", get_balance(AccountId::player(ALICE)));

    assert_eq!(balance_of(&response), 100);
    assert!(economy.accounts().is_empty());
}

#[test]
fn depositing_materializes_a_player_account() {
    let mut economy = economy();
    let response = send(&mut economy, "shop", deposit(AccountId::player(ALICE), 250));

    assert_eq!(balance_of(&response), 350);
    assert!(economy.exists(&AccountKey::Player(ALICE)));
    assert_eq!(economy.seq(), 1);
}

#[test]
fn a_failed_withdrawal_changes_nothing() {
    let mut economy = economy();
    let response = send(
        &mut economy,
        "shop",
        withdraw(AccountId::player(ALICE), 500),
    );

    assert_eq!(error_code(&response), ErrorCode::InsufficientFunds);
    match response.body {
        Some(response::Body::Error(error)) => assert_eq!(error.balance, 100),
        other => panic!("expected an error, got {other:?}"),
    }
    assert_eq!(economy.balance(&AccountKey::Player(ALICE)), 100);
    assert_eq!(economy.seq(), 0);
}

#[test]
fn transfers_move_both_balances_or_neither() {
    let mut economy = economy();
    let response = send(
        &mut economy,
        "shop",
        request::Body::Transfer(Transfer {
            from: Some(AccountId::player(ALICE)),
            to: Some(AccountId::player(BOB)),
            amount: 40,
            reason: "trade".into(),
        }),
    );

    match response.body {
        Some(response::Body::Transfer(result)) => {
            assert_eq!(result.from.unwrap().amount, 60);
            assert_eq!(result.to.unwrap().amount, 140);
        }
        other => panic!("expected a transfer result, got {other:?}"),
    }

    let failed = send(
        &mut economy,
        "shop",
        request::Body::Transfer(Transfer {
            from: Some(AccountId::player(ALICE)),
            to: Some(AccountId::player(BOB)),
            amount: 1_000,
            reason: String::new(),
        }),
    );
    assert_eq!(error_code(&failed), ErrorCode::InsufficientFunds);
    assert_eq!(economy.balance(&AccountKey::Player(ALICE)), 60);
    assert_eq!(economy.balance(&AccountKey::Player(BOB)), 140);
}

#[test]
fn an_account_cannot_pay_itself() {
    let mut economy = economy();
    let response = send(
        &mut economy,
        "shop",
        request::Body::Transfer(Transfer {
            from: Some(AccountId::player(ALICE)),
            to: Some(AccountId::player(ALICE)),
            amount: 10,
            reason: String::new(),
        }),
    );

    assert_eq!(error_code(&response), ErrorCode::InvalidAccount);
}

#[test]
fn named_accounts_must_be_created_before_use() {
    let mut economy = economy();
    let till = AccountId::named("shop", "till");

    let response = send(&mut economy, "shop", deposit(till.clone(), 10));
    assert_eq!(error_code(&response), ErrorCode::AccountNotFound);

    send(&mut economy, "shop", create(till.clone(), Some(500)));
    let response = send(&mut economy, "shop", deposit(till.clone(), 10));
    assert_eq!(balance_of(&response), 510);

    let again = send(&mut economy, "shop", create(till, None));
    assert_eq!(error_code(&again), ErrorCode::AccountExists);
}

#[test]
fn only_the_owner_may_reset_a_named_account() {
    let mut economy = economy();
    let till = AccountId::named("shop", "till");
    send(&mut economy, "shop", create(till.clone(), Some(500)));

    let set = |amount| {
        request::Body::SetBalance(SetBalance {
            account: Some(till.clone()),
            amount,
            reason: String::new(),
        })
    };

    assert_eq!(
        error_code(&send(&mut economy, "casino", set(0))),
        ErrorCode::PermissionDenied
    );
    assert_eq!(balance_of(&send(&mut economy, "shop", set(20))), 20);
    assert_eq!(balance_of(&send(&mut economy, ADMIN_SENDER, set(7))), 7);
}

#[test]
fn anyone_may_pay_into_someone_elses_named_account() {
    let mut economy = economy();
    let till = AccountId::named("shop", "till");
    send(&mut economy, "shop", create(till.clone(), Some(0)));

    assert_eq!(
        balance_of(&send(&mut economy, "casino", deposit(till, 25))),
        25
    );
}

#[test]
fn deleting_a_named_account_needs_ownership_too() {
    let mut economy = economy();
    let till = AccountId::named("shop", "till");
    send(&mut economy, "shop", create(till.clone(), Some(5)));

    let delete = request::Body::DeleteAccount(pvault_proto::DeleteAccount {
        account: Some(till.clone()),
    });
    assert_eq!(
        error_code(&send(&mut economy, "casino", delete.clone())),
        ErrorCode::PermissionDenied
    );

    send(&mut economy, "shop", delete);
    assert!(!economy.exists(&AccountKey::Named("shop:till".into())));
}

#[test]
fn negative_amounts_are_rejected() {
    let mut economy = economy();
    let response = send(&mut economy, "shop", deposit(AccountId::player(ALICE), -1));

    assert_eq!(error_code(&response), ErrorCode::InvalidAmount);
}

#[test]
fn balances_cannot_overflow() {
    let mut economy = economy();
    send(
        &mut economy,
        ADMIN_SENDER,
        request::Body::SetBalance(SetBalance {
            account: Some(AccountId::player(ALICE)),
            amount: i64::MAX,
            reason: String::new(),
        }),
    );

    let response = send(&mut economy, "shop", deposit(AccountId::player(ALICE), 1));
    assert_eq!(error_code(&response), ErrorCode::Overflow);
}

#[test]
fn malformed_account_ids_are_named_as_such() {
    let mut economy = economy();

    let bad_uuid = AccountId {
        kind: Some(pvault_proto::account_id::Kind::Player(vec![0; 4])),
    };
    assert_eq!(
        error_code(&send(&mut economy, "shop", get_balance(bad_uuid))),
        ErrorCode::InvalidAccount
    );

    let bad_name = AccountId {
        kind: Some(pvault_proto::account_id::Kind::Named("no-namespace".into())),
    };
    assert_eq!(
        error_code(&send(&mut economy, "shop", get_balance(bad_name))),
        ErrorCode::InvalidAccount
    );
}

#[test]
fn a_different_major_version_is_turned_away() {
    let economy = economy();
    let request = Request {
        version: Some(Version {
            major: pvault_proto::SPEC_VERSION.major + 1,
            minor: 0,
            patch: 0,
        }),
        body: Some(get_balance(AccountId::player(ALICE))),
    };

    let outcome = economy.plan("shop", 0, &request);
    assert_eq!(error_code(&outcome.response), ErrorCode::VersionMismatch);
    assert!(outcome.entry.is_none());
}

#[test]
fn a_newer_minor_version_is_served() {
    let economy = economy();
    let request = Request {
        version: Some(Version {
            major: pvault_proto::SPEC_VERSION.major,
            minor: pvault_proto::SPEC_VERSION.minor + 5,
            patch: 0,
        }),
        body: Some(get_balance(AccountId::player(ALICE))),
    };

    let outcome = economy.plan("shop", 0, &request);
    assert_eq!(balance_of(&outcome.response), 100);
    assert_eq!(outcome.response.version, Some(pvault_proto::SPEC_VERSION));
}

#[test]
fn a_body_we_do_not_understand_is_reported_clearly() {
    let economy = economy();
    let request = Request {
        version: Some(pvault_proto::SPEC_VERSION),
        body: None,
    };

    assert_eq!(
        error_code(&economy.plan("shop", 0, &request).response),
        ErrorCode::UnknownRequest
    );
}

#[test]
fn recovery_replays_the_log_on_top_of_the_snapshot() {
    let mut economy = economy();
    let mut log = String::new();

    for amount in [10, 20, 30] {
        let outcome = economy.plan(
            "shop",
            0,
            &Request::new(deposit(AccountId::player(ALICE), amount)),
        );
        let entry = outcome.entry.unwrap();
        log.push_str(&encode_log_entry(&entry).unwrap());
        log.push('\n');
        economy.apply(&entry);
    }

    let snapshot = Snapshot::from_json(&economy.snapshot().to_json().unwrap()).unwrap();
    let (recovered, problems) = recover(economy.config().clone(), Some(snapshot), &log);

    assert!(problems.is_empty());
    assert_eq!(recovered.balance(&AccountKey::Player(ALICE)), 160);
    assert_eq!(recovered.seq(), economy.seq());
}

#[test]
fn a_half_written_final_log_line_is_dropped_with_a_warning() {
    let mut economy = economy();
    let mut log = String::new();

    for amount in [10, 20] {
        let outcome = economy.plan(
            "shop",
            0,
            &Request::new(deposit(AccountId::player(ALICE), amount)),
        );
        let entry = outcome.entry.unwrap();
        log.push_str(&encode_log_entry(&entry).unwrap());
        log.push('\n');
        economy.apply(&entry);
    }
    log.push_str("{\"seq\":3,\"time\":0,\"sender\":\"sh");

    let (recovered, problems) = recover(
        Config {
            starting_balance: 100,
            ..Config::default()
        },
        None,
        &log,
    );

    assert_eq!(problems.len(), 1);
    assert_eq!(recovered.balance(&AccountKey::Player(ALICE)), 130);
    assert_eq!(recovered.seq(), 2);
}

#[test]
fn replaying_the_same_entry_twice_is_harmless() {
    let mut economy = economy();
    let outcome = economy.plan(
        "shop",
        0,
        &Request::new(deposit(AccountId::player(ALICE), 40)),
    );
    let entry = outcome.entry.unwrap();

    economy.apply(&entry);
    economy.apply(&entry);

    assert_eq!(economy.balance(&AccountKey::Player(ALICE)), 140);
}
