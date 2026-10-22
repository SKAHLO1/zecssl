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

/// Where to listen: the explicit variable, else Render-style `PORT` on all interfaces, else
/// localhost on the default port.
pub fn listen_addr(var: &str, default_port: u16) -> Result<std::net::SocketAddr> {
    let v = match std::env::var(var).ok().filter(|v| !v.trim().is_empty()) {
        Some(v) => v,
        None => match std::env::var("PORT").ok().filter(|v| !v.trim().is_empty()) {
            Some(port) => format!("0.0.0.0:{}", port.trim()),
            None => format!("127.0.0.1:{default_port}"),
        },
    };
    v.trim().parse().map_err(|e| anyhow!("{var}: {e}"))
}

/// Accepts `host:port` (as Render's service references give it) or a full URL.
pub fn http_url(s: &str) -> String {
    let s = s.trim().trim_end_matches('/');
    if s.contains("://") { s.to_string() } else { format!("http://{s}") }
}

/// Named endpoints from `name=url,name=url`, or, if that is unset, from numbered variables
/// `{prefix}_1_URL` / `{prefix}_1_NAME` ... `{prefix}_9_URL` (one per service reference).
pub fn endpoints(list_var: &str, prefix: &str) -> Result<Vec<(String, String)>> {
    let list = opt(list_var, "");
    if !list.is_empty() {
        return list
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .map(|pair| {
                let (name, url) = pair.split_once('=').ok_or_else(|| anyhow!("{list_var} entries are name=url"))?;
                Ok((name.trim().to_string(), http_url(url)))
            })
            .collect();
    }
    let mut out = vec![];
    for i in 1..=9 {
        if let Some(url) = std::env::var(format!("{prefix}_{i}_URL")).ok().filter(|v| !v.trim().is_empty()) {
            let name = opt(&format!("{prefix}_{i}_NAME"), &format!("operator-{i}"));
            out.push((name, http_url(&url)));
        }
    }
    Ok(out)
}
