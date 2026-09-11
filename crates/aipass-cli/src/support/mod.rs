pub(crate) mod agent;
pub(crate) mod doctor;
pub(crate) mod native_host;
pub(crate) mod parsing;
pub(crate) mod service;
pub(crate) mod snippets;

pub(crate) use agent::*;
pub(crate) use doctor::*;
pub(crate) use native_host::*;
pub(crate) use parsing::*;
pub(crate) use service::*;
pub(crate) use snippets::*;

use anyhow::Result;
use std::path::PathBuf;

pub(crate) fn output(json: bool, value: serde_json::Value, text: &str) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("{text}");
    }
    Ok(())
}

pub(crate) fn absolute_path(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path);
    }
    Ok(std::env::current_dir()?.join(path))
}
