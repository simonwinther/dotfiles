use anyhow::{Context, Result, ensure};
use reqwest::{
    Method,
    blocking::{Client, Response},
    redirect::Policy,
};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::Path, time::Duration};

const BASE: &str = "http://local-tailscaled.sock/localapi/v0/files/";
const MAX_LIST: u64 = 8 * 1024 * 1024;

#[derive(Debug, Deserialize, Serialize)]
pub struct Pending {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Size")]
    pub size: u64,
}

pub struct Api(Client);

impl Api {
    pub fn new(socket: &Path) -> Result<Self> {
        Ok(Self(
            Client::builder()
                .unix_socket(socket.to_path_buf())
                .no_proxy()
                .redirect(Policy::none())
                .timeout(Duration::from_secs(45))
                .build()?,
        ))
    }

    pub fn request(&self, method: Method, suffix: &str, expected: u16) -> Result<Response> {
        let response = self
            .0
            .request(method, format!("{BASE}{suffix}"))
            .send()
            .context("Could not connect to the Tailscale file inbox")?;
        let status = response.status().as_u16();
        if status != expected {
            let mut detail = String::new();
            response.take(2000).read_to_string(&mut detail).ok();
            anyhow::bail!("Tailscale returned {status}: {}", detail.trim());
        }
        Ok(response)
    }

    pub fn pending(&self, wait: bool) -> Result<Vec<Pending>> {
        let response = self.request(
            Method::GET,
            if wait { "?waitsec=30" } else { "?waitsec=0" },
            200,
        )?;
        let mut bytes = Vec::new();
        response.take(MAX_LIST + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_LIST,
            "Tailscale's file list is too large"
        );
        let list: Option<Vec<Pending>> =
            serde_json::from_slice(&bytes).context("Invalid Tailscale file list")?;
        Ok(list.unwrap_or_default())
    }
}

pub fn escaped(name: &str) -> String {
    let mut encoded = String::new();
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(byte as char);
        } else {
            use std::fmt::Write;
            write!(encoded, "%{byte:02X}").unwrap();
        }
    }
    encoded
}
