/* Copyright 2025 The Reflection Developers
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

#[cfg(target_os = "linux")]
use std::collections::HashMap;
use thiserror::Error;
use tracing::info;

#[cfg(target_os = "linux")]
use crate::APP_ID;
use reflection_doc::identity::{IdentityError, SigningKey};

#[cfg(target_os = "linux")]
const XDG_SCHEMA: &str = "xdg:schema";

#[cfg(target_os = "linux")]
fn attributes() -> HashMap<&'static str, String> {
    HashMap::from([(XDG_SCHEMA, APP_ID.to_owned())])
}

#[cfg(target_os = "macos")]
use base64::engine::general_purpose::STANDARD as Base64Engine;

#[cfg(target_os = "macos")]
use base64::Engine as _;

#[derive(Debug, Error)]
pub enum Error {
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    Service(#[from] oo7::Error),
    #[cfg(target_os = "macos")]
    #[error(transparent)]
    Service(#[from] keyring::Error),
    #[error(transparent)]
    Format(#[from] IdentityError),
}

#[cfg(target_os = "linux")]
pub async fn get_or_create_identity() -> Result<SigningKey, Error> {
    let keyring = oo7::Keyring::new().await?;

    keyring.unlock().await?;

    let signing_key: SigningKey =
        if let Some(item) = keyring.search_items(&attributes()).await?.first() {
            item.unlock().await?;
            let signing_key = SigningKey::try_from(item.secret().await?.as_bytes())?;
            info!("Found existing identity: {}", signing_key.verifying_key());

            signing_key
        } else {
            let signing_key = SigningKey::generate();
            keyring
                .create_item("Reflection", &attributes(), signing_key.as_bytes(), true)
                .await?;

            info!(
                "No existing identity found. Create new identity: {}",
                signing_key.verifying_key()
            );
            signing_key
        };

    Ok(signing_key)
}

#[cfg(target_os = "macos")]
pub async fn get_or_create_identity() -> Result<SigningKey, Error> {
    let entry = keyring::Entry::new("Reflection Identity", "default user")?;

    let signing_key: SigningKey = match entry.get_password() {
        Ok(password) => {
            let signing_key = SigningKey::try_from(
                Base64Engine
                    .decode(password)
                    .expect("Failed to decode base64 secret from keyring")
                    .as_slice(),
            )?;
            info!("Found existing identity: {}", signing_key.verifying_key());
            signing_key
        }
        Err(keyring::Error::NoEntry) => {
            let signing_key = SigningKey::generate();
            entry.set_password(&Base64Engine.encode(signing_key.as_bytes()))?;
            info!(
                "No existing identity found. Create new identity: {}",
                signing_key.verifying_key()
            );
            signing_key
        }
        Err(e) => return Err(Error::Service(e)),
    };

    Ok(signing_key)
}
