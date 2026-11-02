use crate::protocol::{self, Activation, Item, Query, QueryResponse, State, StateRequest};
use anyhow::{Result, ensure};
use std::{os::unix::net::UnixStream, path::Path, time::Duration};

pub fn connect(path: &Path) -> Result<UnixStream> {
    let stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    Ok(stream)
}

pub fn query(path: &Path, query: &Query, mut item: impl FnMut(Item) -> Result<()>) -> Result<()> {
    let mut stream = connect(path)?;
    protocol::write_request(&mut stream, protocol::QUERY, 1, query)?;
    loop {
        let (kind, body) = protocol::read_frame(&mut stream)?;
        match kind {
            protocol::ITEM | 1 => {
                if let Some(value) = protocol::decode::<QueryResponse>(1, &body)?.item {
                    item(value)?;
                }
            }
            protocol::DONE => return Ok(()),
            protocol::EMPTY => {}
            _ => anyhow::bail!("Unexpected query response"),
        }
    }
}

pub fn activate(path: &Path, action: &Activation) -> Result<serde_json::Value> {
    let mut stream = connect(path)?;
    protocol::write_request(&mut stream, protocol::ACTIVATE, 1, action)?;
    loop {
        let (kind, body) = protocol::read_frame(&mut stream)?;
        if kind != protocol::ACTIVATED {
            continue;
        }
        let value: serde_json::Value = if body.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_slice(&body)?
        };
        ensure!(
            value.get("error").is_none(),
            "{}",
            value
                .get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Provider action failed")
        );
        return Ok(value);
    }
}

pub fn state(path: &Path, provider: &str) -> Result<State> {
    let mut stream = connect(path)?;
    protocol::write_request(
        &mut stream,
        protocol::STATE,
        1,
        &StateRequest {
            provider: provider.into(),
        },
    )?;
    let (kind, body) = protocol::read_frame(&mut stream)?;
    ensure!(
        kind == protocol::PROVIDER_STATE,
        "Unexpected provider state response"
    );
    protocol::decode(1, &body)
}
