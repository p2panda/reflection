/* Copyright 2025 The Aardvark Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

// #[cfg(not(target_os = "macos"))]
#[cfg(target_os = "linux")]
use std::collections::HashMap;
use thiserror::Error;
use tracing::info;

#[cfg(not(target_os = "macos"))]
use crate::APP_ID;
use aardvark_doc::identity::{IdentityError, PrivateKey};

#[cfg(not(target_os = "macos"))]
const XDG_SCHEMA: &'static str = "xdg:schema";

#[cfg(not(target_os = "macos"))]
fn attributes() -> HashMap<&'static str, String> {
    HashMap::from([(XDG_SCHEMA, APP_ID.to_owned())])
}

#[cfg(target_os = "macos")]
use base64::engine::general_purpose::STANDARD as Base64Engine;

#[cfg(target_os = "macos")]
use base64::Engine as _;

#[derive(Debug, Error)]
pub enum Error {
    #[cfg(not(target_os = "macos"))]
    #[error("Secret Service error: {0}")]
    Service(oo7::Error),
    #[cfg(target_os = "macos")]
    #[error("Keyring error: {0}")]
    Service(keyring::Error),
    #[error("Format error: {0}")]
    Format(IdentityError),
    #[error("Base64 decode error: {0}")]
    Base64Decode(base64::DecodeError),
}

impl From<IdentityError> for Error {
    fn from(value: IdentityError) -> Self {
        Error::Format(value)
    }
}

impl From<base64::DecodeError> for Error {
    fn from(value: base64::DecodeError) -> Self {
        Error::Base64Decode(value)
    }
}

#[cfg(not(target_os = "macos"))]
impl From<oo7::Error> for Error {
    fn from(value: oo7::Error) -> Self {
        Error::Service(value)
    }
}

#[cfg(not(target_os = "macos"))]
pub async fn get_or_create_identity() -> Result<PrivateKey, Error> {
    let keyring = oo7::Keyring::new().await?;

    keyring.unlock().await?;

    let private_key: PrivateKey =
        if let Some(item) = keyring.search_items(&attributes()).await?.get(0) {
            item.unlock().await?;
            let private_key = PrivateKey::try_from(item.secret().await?.as_bytes())?;
            info!("Found existing identity: {}", private_key.public_key());

            private_key
        } else {
            let private_key = PrivateKey::new();
            keyring
                .create_item("Aardvark", &attributes(), private_key.as_bytes(), true)
                .await?;

            info!(
                "No existing identity found. Create new identity: {}",
                private_key.public_key()
            );
            private_key
        };

    Ok(private_key)
}

#[cfg(target_os = "macos")]
pub async fn get_or_create_identity() -> Result<PrivateKey, Error> {
    let entry = keyring::Entry::new("Aardvark Identity", "default user").map_err(Error::Service)?;

    let private_key: PrivateKey = match entry.get_password() {
        Ok(password) => {
            let private_key = PrivateKey::try_from(Base64Engine.decode(password)?.as_slice())?;
            info!("Found existing identity: {}", private_key.public_key());
            private_key
        }
        Err(keyring::Error::NoEntry) => {
            let private_key = PrivateKey::new();
            entry
                .set_password(&Base64Engine.encode(private_key.as_bytes()))
                .map_err(Error::Service)?;
            info!(
                "No existing identity found. Create new identity: {}",
                private_key.public_key()
            );
            private_key
        }
        Err(e) => return Err(Error::Service(e)),
    };

    Ok(private_key)
}
