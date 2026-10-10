//! Official CLIs own grants. This facade exposes only local references and operations.
use super::*;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

mod backend;
mod discovery;
mod quota;
mod session;
mod store;

#[cfg(test)]
use backend::copilot_user_request;
pub(super) use backend::{generate, models, usage};
pub(super) use discovery::executable;
pub(crate) use discovery::status;
use quota::{codex_usage, copilot_usage};
use session::{command, rpc};
pub(super) use session::{fresh, login, verify};
#[cfg(test)]
use store::config_home;
pub(crate) use store::{
    check_device, default_home, device, handoff_home, import_reference, new_home, reference,
};
use store::{read, root};

#[cfg(test)]
mod tests;
