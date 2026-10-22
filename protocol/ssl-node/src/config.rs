//! Environment configuration. Every setting has a name; only the ones without a sensible
//! default are required.

use anyhow::{anyhow, Context, Result};

pub fn req(name: &str) -> Result<String> {
    std::env::var(name).map(|v| v.trim().to_string()).ok().filter(|v| !v.is_empty()).ok_or_else(|| anyhow!("{name} is required"))
}

pub fn opt(name: &str, default: &str) -> String {
    std::env::var(name).map(|v| v.trim().to_string()).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| default.to_string())
}

pub fn num<T: std::str::FromStr>(name: &str, default: T) -> Result<T>
where
    T::Err: std::fmt::Display,
{
    match std::env::var(name).ok().filter(|v| !v.trim().is_empty()) {
        Some(v) => v.trim().parse().map_err(|e| anyhow!("{name}: {e}")),
        None => Ok(default),
    }
}

/// A value, or the contents of a file when the value starts with `@` (for keys kept out of
/// the environment, e.g. `TREASURY_UFVK=@/etc/zec-ssl/ufvk.txt`).
pub fn value_or_file(name: &str) -> Result<String> {
    let v = req(name)?;
    match v.strip_prefix('@') {
        Some(path) => Ok(std::fs::read_to_string(path).with_context(|| format!("{name}: reading {path}"))?.trim().to_string()),
        None => Ok(v),
    }
}
