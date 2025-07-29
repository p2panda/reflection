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

use thiserror::Error;

use reflection_doc::identity::{IdentityError, PrivateKey};

const ALICE_PRIVATE_KEY: &str = "c13ae3388b1d99d27daba169af73b294537634f7fb8b9789c409c5874c4043b5";

const BOB_PRIVATE_KEY: &str = "f0849b0b8b3d1702e7d8cd470ebe2e3446337c08f9d34b4b86d238d3a2ff06ea";

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Service(#[from] oo7::Error),

    #[error(transparent)]
    Format(#[from] IdentityError),
}

pub async fn get_or_create_identity() -> Result<PrivateKey, Error> {
    let Ok(id) = std::env::var("SPACES_PEER_ID") else {
        panic!(
            "this is an experimental version of reflection with p2panda-spaces integration. You
            _need_ to set a SPACES_PEER_ID env var"
        );
    };

    let private_key = match id.as_str() {
        "alice" => ALICE_PRIVATE_KEY.parse().expect("correct private key"),
        "bob" => BOB_PRIVATE_KEY.parse().expect("correct private key"),
        _ => {
            panic!("unknown SPACES_PEER_ID value, we only know alice or bob")
        }
    };

    Ok(private_key)
}
