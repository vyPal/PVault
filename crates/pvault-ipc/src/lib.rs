// SPDX-License-Identifier: 0BSD

//! Talk to the PVault economy from another Pumpkin plugin.
//!
//! ```rust,ignore
//! use pvault_ipc::{Economy, account};
//!
//! let player = account::player(&player.get_id());
//! if Economy::has(&player, 500)? {
//!     Economy::withdraw(&player, 500, "bought a diamond pickaxe")?;
//! }
//! ```
//!
//! Every call is a synchronous host round-trip to the plugin named `PVault`.

use pumpkin_plugin_api::{ipc, uuid::Uuid};
use pvault_proto::{
    AccountId, AccountStatus, Balance, CurrencyInfo, ErrorCode, Message, Request, Response,
    TransferResult, Version, request, response,
};

pub use pvault_proto::{PLUGIN_NAME, SPEC_VERSION};

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// PVault is not installed, not loaded, or the host refused to route the message.
    Unreachable,
    /// PVault rejected the bytes outright rather than answering.
    Transport(String),
    /// The reply was not a `pvault.economy.v1.Response`.
    Decode(String),
    /// PVault answered, but with a different major spec version than this crate speaks.
    Incompatible(Version),
    /// A normal economy failure: insufficient funds, unknown account, and so on.
    Failed {
        code: ErrorCode,
        message: String,
        /// Balance of the offending account. Only meaningful for insufficient funds.
        balance: i64,
    },
    /// PVault answered with a body that doesn't match the request.
    Unexpected,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable => write!(f, "{PLUGIN_NAME} is not available"),
            Self::Transport(message) => write!(f, "{PLUGIN_NAME} refused the message: {message}"),
            Self::Decode(message) => write!(f, "could not read the reply: {message}"),
            Self::Incompatible(version) => write!(
                f,
                "{PLUGIN_NAME} speaks {version}, this plugin was built against {SPEC_VERSION}"
            ),
            Self::Failed { message, .. } => write!(f, "{message}"),
            Self::Unexpected => write!(f, "{PLUGIN_NAME} answered a different question"),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    /// Whether the failure was specifically an empty wallet.
    #[must_use]
    pub const fn is_insufficient_funds(&self) -> bool {
        matches!(
            self,
            Self::Failed {
                code: ErrorCode::InsufficientFunds,
                ..
            }
        )
    }
}

/// Account id constructors.
pub mod account {
    use super::{AccountId, Uuid};

    #[must_use]
    pub fn player(uuid: &Uuid) -> AccountId {
        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&uuid.high.to_be_bytes());
        bytes[8..].copy_from_slice(&uuid.low.to_be_bytes());
        AccountId::player(bytes)
    }

    /// A named account. Use your own plugin name as the namespace.
    #[must_use]
    pub fn named(namespace: &str, key: &str) -> AccountId {
        AccountId::named(namespace, key)
    }
}

pub struct Economy;

impl Economy {
    /// Name, symbol and precision of the server's currency. Worth caching on load.
    ///
    /// # Errors
    /// If PVault is unreachable or answers with an error.
    pub fn currency() -> Result<CurrencyInfo> {
        match send(request::Body::GetCurrency(pvault_proto::GetCurrency {}))? {
            response::Body::Currency(info) => Ok(info),
            _ => Err(Error::Unexpected),
        }
    }

    /// # Errors
    /// If PVault is unreachable or the account is unknown.
    pub fn balance(account: &AccountId) -> Result<i64> {
        Self::balance_of(request::Body::GetBalance(pvault_proto::GetBalance {
            account: Some(account.clone()),
        }))
        .map(|balance| balance.amount)
    }

    /// # Errors
    /// If PVault is unreachable or the account is unknown.
    pub fn has(account: &AccountId, amount: i64) -> Result<bool> {
        match send(request::Body::HasBalance(pvault_proto::HasBalance {
            account: Some(account.clone()),
            amount,
        }))? {
            response::Body::HasBalance(result) => Ok(result.sufficient),
            _ => Err(Error::Unexpected),
        }
    }

    /// # Errors
    /// If PVault is unreachable, the account is unknown, or the balance would overflow.
    pub fn deposit(account: &AccountId, amount: i64, reason: &str) -> Result<Balance> {
        Self::balance_of(request::Body::Deposit(pvault_proto::Deposit {
            account: Some(account.clone()),
            amount,
            reason: reason.to_owned(),
        }))
    }

    /// # Errors
    /// If PVault is unreachable, the account is unknown, or it cannot cover `amount`.
    pub fn withdraw(account: &AccountId, amount: i64, reason: &str) -> Result<Balance> {
        Self::balance_of(request::Body::Withdraw(pvault_proto::Withdraw {
            account: Some(account.clone()),
            amount,
            reason: reason.to_owned(),
        }))
    }

    /// Moves money between two accounts, or changes neither.
    ///
    /// # Errors
    /// If PVault is unreachable, either account is unknown, or `from` cannot cover `amount`.
    pub fn transfer(
        from: &AccountId,
        to: &AccountId,
        amount: i64,
        reason: &str,
    ) -> Result<TransferResult> {
        match send(request::Body::Transfer(pvault_proto::Transfer {
            from: Some(from.clone()),
            to: Some(to.clone()),
            amount,
            reason: reason.to_owned(),
        }))? {
            response::Body::Transfer(result) => Ok(result),
            _ => Err(Error::Unexpected),
        }
    }

    /// Only the plugin that created a named account may set its balance.
    ///
    /// # Errors
    /// If PVault is unreachable, the account is unknown, or it belongs to another plugin.
    pub fn set_balance(account: &AccountId, amount: i64, reason: &str) -> Result<Balance> {
        Self::balance_of(request::Body::SetBalance(pvault_proto::SetBalance {
            account: Some(account.clone()),
            amount,
            reason: reason.to_owned(),
        }))
    }

    /// Creates an account. Named accounts must exist before they can be used.
    ///
    /// # Errors
    /// If PVault is unreachable or the account already exists.
    pub fn create_account(account: &AccountId, initial_balance: Option<i64>) -> Result<i64> {
        Self::status(request::Body::CreateAccount(pvault_proto::CreateAccount {
            account: Some(account.clone()),
            initial_balance,
        }))
        .map(|status| status.balance)
    }

    /// # Errors
    /// If PVault is unreachable.
    pub fn exists(account: &AccountId) -> Result<bool> {
        Self::status(request::Body::AccountExists(pvault_proto::AccountExists {
            account: Some(account.clone()),
        }))
        .map(|status| status.exists)
    }

    /// # Errors
    /// If PVault is unreachable, the account is unknown, or it belongs to another plugin.
    pub fn delete_account(account: &AccountId) -> Result<()> {
        Self::status(request::Body::DeleteAccount(pvault_proto::DeleteAccount {
            account: Some(account.clone()),
        }))
        .map(|_| ())
    }

    fn balance_of(body: request::Body) -> Result<Balance> {
        match send(body)? {
            response::Body::Balance(balance) => Ok(balance),
            _ => Err(Error::Unexpected),
        }
    }

    fn status(body: request::Body) -> Result<AccountStatus> {
        match send(body)? {
            response::Body::Account(status) => Ok(status),
            _ => Err(Error::Unexpected),
        }
    }
}

fn send(body: request::Body) -> Result<response::Body> {
    let message = Request::new(body).encode_to_vec();
    let reply = match ipc::send_ipc_message(PLUGIN_NAME, &message) {
        Ok(Ok(reply)) => reply,
        Ok(Err(message)) => return Err(Error::Transport(message)),
        Err(()) => return Err(Error::Unreachable),
    };

    let response = Response::decode(reply.as_slice()).map_err(|e| Error::Decode(e.to_string()))?;
    if let Some(version) = response.version
        && !SPEC_VERSION.is_compatible_with(&version)
    {
        return Err(Error::Incompatible(version));
    }

    match response.body {
        Some(response::Body::Error(error)) => Err(Error::Failed {
            code: ErrorCode::try_from(error.code).unwrap_or(ErrorCode::Unspecified),
            message: error.message,
            balance: error.balance,
        }),
        Some(body) => Ok(body),
        None => Err(Error::Unexpected),
    }
}
