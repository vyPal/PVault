// SPDX-License-Identifier: 0BSD

//! Message types for the PVault economy IPC API.
//!
//! The normative definition lives in `proto/pvault/economy/v1/economy.proto`; the Rust
//! bindings here are generated from it and checked in, so nothing needs `protoc` to build.
//! Run `cargo run -p proto-gen --target <host>` after editing the `.proto`.

#![allow(clippy::doc_markdown)]

pub mod economy {
    pub mod v1 {
        include!("generated/pvault.economy.v1.rs");
    }
}

pub use economy::v1::*;
pub use prost::Message;

/// The plugin name PVault is registered under, and therefore the IPC recipient id.
pub const PLUGIN_NAME: &str = "PVault";

/// The spec version implemented by this crate. Tracks the PVault plugin version.
pub const SPEC_VERSION: Version = Version {
    major: parse_u32(env!("CARGO_PKG_VERSION_MAJOR")),
    minor: parse_u32(env!("CARGO_PKG_VERSION_MINOR")),
    patch: parse_u32(env!("CARGO_PKG_VERSION_PATCH")),
};

const fn parse_u32(s: &str) -> u32 {
    let bytes = s.as_bytes();
    let mut value = 0u32;
    let mut i = 0;
    while i < bytes.len() {
        assert!(
            bytes[i].is_ascii_digit(),
            "version component must be numeric"
        );
        value = value * 10 + (bytes[i] - b'0') as u32;
        i += 1;
    }
    value
}

impl Version {
    /// Whether a peer speaking `self` can talk to a peer speaking `other`.
    ///
    /// Only the major version has to match; a differing minor or patch is served normally.
    #[must_use]
    pub const fn is_compatible_with(&self, other: &Self) -> bool {
        self.major == other.major
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl AccountId {
    /// An account belonging to a player, identified by their UUID.
    #[must_use]
    pub fn player(uuid: [u8; 16]) -> Self {
        Self {
            kind: Some(account_id::Kind::Player(uuid.to_vec())),
        }
    }

    /// A named account such as a shop till or town treasury.
    ///
    /// `namespace` should be your plugin's name so keys can't collide between plugins.
    #[must_use]
    pub fn named(namespace: &str, key: &str) -> Self {
        Self {
            kind: Some(account_id::Kind::Named(format!("{namespace}:{key}"))),
        }
    }
}

impl Error {
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code: code as i32,
            message: message.into(),
            balance: 0,
        }
    }
}

impl Request {
    #[must_use]
    pub fn new(body: request::Body) -> Self {
        Self {
            version: Some(SPEC_VERSION),
            body: Some(body),
        }
    }
}

impl Response {
    #[must_use]
    pub fn new(body: response::Body) -> Self {
        Self {
            version: Some(SPEC_VERSION),
            body: Some(body),
        }
    }

    #[must_use]
    pub fn error(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::new(response::Body::Error(Error::new(code, message)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_version_matches_crate_version() {
        assert_eq!(SPEC_VERSION.to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn round_trips_a_request() {
        let request = Request::new(request::Body::Deposit(Deposit {
            account: Some(AccountId::player([7; 16])),
            amount: 250,
            reason: "sold 3 diamonds".into(),
        }));

        let decoded = Request::decode(request.encode_to_vec().as_slice()).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn only_major_version_gates_compatibility() {
        let ours = Version {
            major: 1,
            minor: 4,
            patch: 0,
        };
        assert!(ours.is_compatible_with(&Version {
            major: 1,
            minor: 0,
            patch: 9
        }));
        assert!(!ours.is_compatible_with(&Version {
            major: 2,
            minor: 4,
            patch: 0
        }));
    }
}
