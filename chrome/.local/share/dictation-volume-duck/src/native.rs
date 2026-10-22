use crate::{Action, Request, factor, run, state};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    time::Duration,
};

const MAX_MESSAGE: usize = 65_536;

fn read_message(reader: &mut impl Read) -> Result<Value> {
    let mut length = [0; 4];
    reader
        .read_exact(&mut length)
        .context("Native message header ended early")?;
    let length = u32::from_ne_bytes(length) as usize;
    ensure!(length <= MAX_MESSAGE, "Native message is too large");
    let mut bytes = vec![0; length];
    reader
        .read_exact(&mut bytes)
        .context("Native message ended early")?;
    serde_json::from_slice(&bytes).context("Invalid native message JSON")
}

fn text(value: Option<&Value>, default: &str) -> String {
    match value {
        None | Some(Value::Null | Value::Bool(false)) => default.into(),
        Some(Value::String(s)) if s.is_empty() => default.into(),
        Some(Value::Number(n)) if n.as_f64() == Some(0.) => default.into(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    }
}

fn respond(message: Value) -> Result<Value> {
    ensure!(message.is_object(), "Native message must be an object");
    let action = text(message.get("action"), "").trim().to_lowercase();
    let action = match action.as_str() {
        "start" => Action::Start,
        "stop" => Action::Stop,
        "reset" => Action::Reset,
        "status" => Action::Status,
        _ => return Ok(json!({"ok":false,"error":format!("Unsupported action: {action}")})),
    };
    let factor = if matches!(action, Action::Start) {
        factor(&text(message.get("factor"), "0.3"))?
    } else {
        0.3
    };
    let request = Request {
        action,
        client: state::sanitize_client(&text(message.get("client"), "chatgpt-dictate")),
        factor,
    };
    let outcome = run(request, Duration::from_secs(5));
    let mut response = json!({"ok":outcome.code == 0,"status":outcome.code});
    if !outcome.stdout.is_empty() {
        response["stdout"] = outcome.stdout.trim().into();
    }
    if !outcome.stderr.is_empty() {
        response["stderr"] = outcome.stderr.trim().into();
    }
    Ok(response)
}

// sendNativeMessage launches a new process for each request; reply once and exit.
pub fn serve(reader: &mut impl Read, writer: &mut impl Write) -> Result<()> {
    let response = match read_message(reader).and_then(respond) {
        Ok(response) => response,
        Err(error) => json!({"ok":false,"error":format!("{error:#}")}),
    };
    let bytes = serde_json::to_vec(&response)?;
    writer.write_all(&(bytes.len() as u32).to_ne_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}
