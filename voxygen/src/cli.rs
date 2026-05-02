//! NOTE: Some of these arguments may still be consumed by legacy external
//! tooling such as Airshipper, so those should be kept fairly stable (probably
//! with some sort of migration period if we need to modify the name or
//! semantics).
//!
//! The arguments that external launch tooling should treat as compatibility
//! surface are:
//! * `server`
//!
//! External tooling should only use arguments listed above. We will not try to
//! preserve stability for the rest.
//!
//! Note that `server` is now a development-oriented override. Public mode no
//! longer treats it as a Caldrayne Realm selector, and realm targeting must
//! continue to flow through `public_realm -> EntryPolicy -> Public / Dev`.
//!
//! Likewise external launch tooling should only use the following subcommands:
//! * `ListWgpuBackends`
use std::str::FromStr;

use clap::{Parser, Subcommand, ValueEnum};
use common_net::msg::ClientType;

#[derive(Parser, Clone)]
pub struct Args {
    /// Development-only value to auto-fill into the server field.
    ///
    /// This preserves legacy compatibility with Airshipper or local tooling
    /// while using developer mode. Public mode ignores this as a Caldrayne
    /// Realm selector and continues to resolve the target through the current
    /// bundled Caldrayne Realm source.
    #[clap(short, long)]
    pub server: Option<String>,

    /// Controls whether the client runs in public mode or developer mode.
    ///
    /// When omitted, debug builds default to `dev` and release builds default
    /// to `public`.
    #[clap(long, env = "CALDRAYNE_PRODUCT_MODE", value_enum)]
    pub product_mode: Option<ProductModeArg>,

    /// The [`ClientType`] the client will use to initialize the connection.
    ///
    /// The only supported values are currently `game` and `silent_spectator`,
    /// the latter one only being usable by moderators.
    #[clap(short, long, env = "CALDRAYNE_CLIENT_TYPE", default_value_t = VoxygenClientType(ClientType::Game))]
    pub client_type: VoxygenClientType,

    #[clap(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Clone)]
pub enum Commands {
    /// List available wgpu backends. This is called by Airshipper to show a
    /// dropbox of available backends.
    ListWgpuBackends,
    /// List available wgpu devices. This is called by Airshipper to show a
    /// dropbox of available devices.
    ListWgpuDevices,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ProductModeArg {
    Public,
    Dev,
}

#[derive(Clone)]
pub struct VoxygenClientType(pub ClientType);

impl FromStr for VoxygenClientType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(match s.to_lowercase().as_str() {
            "game" => ClientType::Game,
            "silent_spectator" => ClientType::SilentSpectator,
            c_type => return Err(format!("Invalid client type: {c_type}")),
        }))
    }
}

impl std::fmt::Display for VoxygenClientType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self.0 {
            ClientType::Game => "game",
            ClientType::ChatOnly => "chat_only",
            ClientType::SilentSpectator => "silent_spectator",
            ClientType::Bot { .. } => "bot",
        })
    }
}
