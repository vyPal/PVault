// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;

use pvault_proto::{AccountId, account_id};
use serde::{Deserialize, Serialize};

pub const MAX_NAMED_LENGTH: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AccountKey {
    Player([u8; 16]),
    Named(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum InvalidAccount {
    Missing,
    BadUuid,
    BadName,
}

impl fmt::Display for InvalidAccount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => write!(f, "no account was given"),
            Self::BadUuid => write!(f, "player accounts are identified by a 16-byte uuid"),
            Self::BadName => write!(
                f,
                "named accounts look like \"namespace:key\", up to {MAX_NAMED_LENGTH} characters of [a-zA-Z0-9_.-]"
            ),
        }
    }
}

impl AccountKey {
    pub fn from_proto(id: Option<&AccountId>) -> Result<Self, InvalidAccount> {
        match id.and_then(|id| id.kind.as_ref()) {
            Some(account_id::Kind::Player(bytes)) => bytes
                .as_slice()
                .try_into()
                .map(Self::Player)
                .map_err(|_| InvalidAccount::BadUuid),
            Some(account_id::Kind::Named(name)) => {
                if is_valid_name(name) {
                    Ok(Self::Named(name.clone()))
                } else {
                    Err(InvalidAccount::BadName)
                }
            }
            None => Err(InvalidAccount::Missing),
        }
    }

    #[must_use]
    pub fn to_proto(&self) -> AccountId {
        match self {
            Self::Player(uuid) => AccountId::player(*uuid),
            Self::Named(name) => AccountId {
                kind: Some(account_id::Kind::Named(name.clone())),
            },
        }
    }

    #[must_use]
    pub const fn is_player(&self) -> bool {
        matches!(self, Self::Player(_))
    }

    #[must_use]
    pub fn storage_key(&self) -> String {
        match self {
            Self::Player(uuid) => uuid.iter().map(|b| format!("{b:02x}")).collect(),
            Self::Named(name) => name.clone(),
        }
    }

    pub fn from_storage_key(key: &str) -> Result<Self, InvalidAccount> {
        if key.contains(':') {
            return if is_valid_name(key) {
                Ok(Self::Named(key.to_owned()))
            } else {
                Err(InvalidAccount::BadName)
            };
        }
        if key.len() != 32 {
            return Err(InvalidAccount::BadUuid);
        }
        let mut uuid = [0u8; 16];
        for (i, byte) in uuid.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&key[i * 2..i * 2 + 2], 16)
                .map_err(|_| InvalidAccount::BadUuid)?;
        }
        Ok(Self::Player(uuid))
    }
}

impl fmt::Display for AccountKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.storage_key())
    }
}

impl Serialize for AccountKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.storage_key())
    }
}

impl<'de> Deserialize<'de> for AccountKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let key = String::deserialize(deserializer)?;
        Self::from_storage_key(&key).map_err(serde::de::Error::custom)
    }
}

fn is_valid_name(name: &str) -> bool {
    if name.len() > MAX_NAMED_LENGTH {
        return false;
    }
    let Some((namespace, key)) = name.split_once(':') else {
        return false;
    };
    !namespace.is_empty()
        && !key.is_empty()
        && !key.contains(':')
        && [namespace, key]
            .iter()
            .all(|part| part.bytes().all(is_name_byte))
}

const fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-')
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub balance: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_well_formed_names() {
        assert!(is_valid_name("shop:till.main"));
        assert!(is_valid_name("towny:town-42"));
    }

    #[test]
    fn rejects_malformed_names() {
        assert!(!is_valid_name("nocolon"));
        assert!(!is_valid_name(":key"));
        assert!(!is_valid_name("ns:"));
        assert!(!is_valid_name("ns:a:b"));
        assert!(!is_valid_name("ns:has space"));
        assert!(!is_valid_name(&format!(
            "ns:{}",
            "x".repeat(MAX_NAMED_LENGTH)
        )));
    }

    #[test]
    fn storage_keys_round_trip() {
        for key in [
            AccountKey::Player([0xab; 16]),
            AccountKey::Named("shop:till".into()),
        ] {
            assert_eq!(AccountKey::from_storage_key(&key.storage_key()), Ok(key));
        }
    }

    #[test]
    fn rejects_wrong_length_uuids() {
        let id = AccountId {
            kind: Some(account_id::Kind::Player(vec![1, 2, 3])),
        };
        assert_eq!(
            AccountKey::from_proto(Some(&id)),
            Err(InvalidAccount::BadUuid)
        );
    }
}
