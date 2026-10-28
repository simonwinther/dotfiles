use crate::{
    cache,
    schema::{EXTENSION_ORIGIN, MAX_MESSAGE, Snapshot, Status, invalid},
};
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    io::{self, Read, Write},
    path::Path,
};

fn receive(origin: Option<&OsStr>, path: &Path, source: &mut impl Read) -> io::Result<Value> {
    // Reject the origin before reading or allocating the browser's message.
    if origin != Some(OsStr::new(EXTENSION_ORIGIN)) {
        return Err(invalid("Unexpected extension origin"));
    }
    let mut header = [0; 4];
    source.read_exact(&mut header)?;
    let size = u32::from_ne_bytes(header) as usize;
    if size == 0 || size > MAX_MESSAGE {
        return Err(invalid("Native message exceeds the size limit"));
    }
    let mut data = vec![0; size];
    source.read_exact(&mut data)?;
    let value: Value = serde_json::from_slice(&data)?;
    if value.get("action").and_then(Value::as_str) == Some("status") {
        let status = Status::from_message(&value, cache::now())?;
        cache::save_status(&status, path)?;
        Ok(json!({"ok": true}))
    } else {
        let snapshot: Snapshot = serde_json::from_value(value)?;
        Ok(match cache::save_snapshot(snapshot, path)? {
            Some(count) => json!({"ok": true, "updated": true, "count": count}),
            None => json!({"ok": true, "updated": false}),
        })
    }
}

/// One request and one framed reply, with no diagnostic text on stdout.
pub fn serve(
    origin: Option<&OsStr>,
    path: &Path,
    source: &mut impl Read,
    destination: &mut impl Write,
) -> io::Result<bool> {
    let result = receive(origin, path, source).unwrap_or_else(
        |_| json!({"ok": false, "error": "Could not save a complete project snapshot"}),
    );
    let data = serde_json::to_vec(&result)?;
    destination.write_all(&(data.len() as u32).to_ne_bytes())?;
    destination.write_all(&data)?;
    destination.flush()?;
    Ok(result["ok"] == true)
}
