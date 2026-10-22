//! ZEC/USD from CoinGecko, cached. Each operator runs its own, so a coordinator cannot
//! feed operators a price.

use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::Mutex;

pub struct PriceFeed {
    http: reqwest::Client,
    url: String,
    max_age: Duration,
    cached: Mutex<Option<(f64, Instant)>>,
}

impl PriceFeed {
    pub fn coingecko() -> Self {
        Self {
            // CoinGecko rejects requests without a User-Agent (403).
            http: reqwest::Client::builder().user_agent(concat!("zec-ssl-node/", env!("CARGO_PKG_VERSION"))).build().expect("static client config"),
            url: "https://api.coingecko.com/api/v3/simple/price?ids=zcash&vs_currencies=usd".into(),
            max_age: Duration::from_secs(60),
            cached: Mutex::new(None),
        }
    }

    /// The current price, or `None` if it cannot be fetched and the cached one is older
    /// than five minutes. Operators refuse to approve without a price.
    pub async fn zec_usd(&self) -> Option<f64> {
        let mut cached = self.cached.lock().await;
        if let Some((p, at)) = *cached {
            if at.elapsed() < self.max_age {
                return Some(p);
            }
        }
        match self.fetch().await {
            Ok(p) => {
                *cached = Some((p, Instant::now()));
                Some(p)
            }
            Err(e) => {
                tracing::warn!(error = %e, "price fetch failed");
                cached.filter(|(_, at)| at.elapsed() < Duration::from_secs(300)).map(|(p, _)| p)
            }
        }
    }

    async fn fetch(&self) -> anyhow::Result<f64> {
        let v: Value = self.http.get(&self.url).timeout(Duration::from_secs(10)).send().await?.error_for_status()?.json().await?;
        v["zcash"]["usd"].as_f64().filter(|p| *p > 0.0).ok_or_else(|| anyhow::anyhow!("no zcash price in response"))
    }
}
