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

use std::collections::HashMap;
use thiserror::Error;
use tracing::info;

use crate::APP_ID;
use reflection_doc::identity::{IdentityError, PrivateKey};

const XDG_SCHEMA: &'static str = "xdg:schema";

fn attributes() -> HashMap<&'static str, String> {
    HashMap::from([(XDG_SCHEMA, APP_ID.to_owned())])
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Secret Service error: {0}")]
    Service(oo7::Error),
    #[error("Format error: {0}")]
    Format(IdentityError),
}

impl From<IdentityError> for Error {
    fn from(value: IdentityError) -> Self {
        Error::Format(value)
    }
}

impl From<oo7::Error> for Error {
    fn from(value: oo7::Error) -> Self {
        Error::Service(value)
    }
}

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
                .create_item("Reflection", &attributes(), private_key.as_bytes(), true)
                .await?;

            info!(
                "No existing identity found. Create new identity: {}",
                private_key.public_key()
            );
            private_key
        };

    Ok(private_key)
}
