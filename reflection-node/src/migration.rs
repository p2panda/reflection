use std::path::PathBuf;

use p2panda::Node;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MigrationError {}

pub async fn run_p2panda_migrations(
    node: &Node,
    base_path: &PathBuf,
) -> Result<(), MigrationError> {
    Ok(())
}
