use anyhow::{Result, bail, ensure};
use prost::Message;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::{Read, Write};

pub const QUERY: u8 = 0;
pub const ACTIVATE: u8 = 1;
pub const SUBSCRIBE: u8 = 2;
pub const MENU: u8 = 3;
pub const STATE: u8 = 4;
pub const ITEM: u8 = 0;
pub const ACTIVATED: u8 = 2;
pub const PROVIDER_STATE: u8 = 3;
pub const STATUS_DONE: u8 = 253;
pub const EMPTY: u8 = 254;
pub const DONE: u8 = 255;
pub const MAX_REQUEST: usize = 1024 * 1024;

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
#[serde(default)]
pub struct Query {
    #[prost(string, repeated, tag = "1")]
    pub providers: Vec<String>,
    #[prost(string, tag = "2")]
    pub query: String,
    #[prost(int32, tag = "3")]
    pub maxresults: i32,
    #[prost(bool, tag = "4")]
    pub exactsearch: bool,
    #[prost(string, tag = "5")]
    pub mode: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
#[serde(default)]
pub struct Item {
    #[prost(string, tag = "1")]
    pub identifier: String,
    #[prost(string, tag = "2")]
    pub text: String,
    #[prost(string, tag = "3")]
    pub subtext: String,
    #[prost(string, tag = "4")]
    pub icon: String,
    #[prost(string, tag = "5")]
    pub provider: String,
    #[prost(int32, tag = "6")]
    pub score: i32,
    #[prost(message, optional, tag = "7")]
    pub fuzzyinfo: Option<FuzzyInfo>,
    #[prost(int32, tag = "8")]
    pub r#type: i32,
    #[prost(string, tag = "9")]
    pub mimetype: String,
    #[prost(string, tag = "10")]
    pub preview: String,
    #[prost(string, tag = "11")]
    pub preview_type: String,
    #[prost(string, repeated, tag = "12")]
    pub state: Vec<String>,
    #[prost(string, repeated, tag = "13")]
    pub actions: Vec<String>,
    #[prost(string, tag = "14")]
    pub payload_path: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
#[serde(default)]
pub struct FuzzyInfo {
    #[prost(int32, tag = "1")]
    pub start: i32,
    #[prost(string, tag = "2")]
    pub field: String,
    #[prost(int32, repeated, tag = "3")]
    pub positions: Vec<i32>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
#[serde(default)]
pub struct QueryResponse {
    #[prost(string, tag = "1")]
    pub query: String,
    #[prost(message, optional, tag = "2")]
    pub item: Option<Item>,
    #[prost(int32, tag = "3")]
    pub qid: i32,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
#[serde(default)]
pub struct Activation {
    #[prost(string, tag = "1")]
    pub provider: String,
    #[prost(string, tag = "2")]
    pub identifier: String,
    #[prost(string, tag = "3")]
    pub action: String,
    #[prost(string, tag = "4")]
    pub query: String,
    #[prost(string, tag = "5")]
    pub arguments: String,
    #[prost(bool, tag = "6")]
    pub single: bool,
    #[prost(string, tag = "7")]
    pub context: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
#[serde(default)]
pub struct StateRequest {
    #[prost(string, tag = "1")]
    pub provider: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    #[prost(string, repeated, tag = "1")]
    pub states: Vec<String>,
    #[prost(string, repeated, tag = "2")]
    pub actions: Vec<String>,
    #[prost(string, tag = "3")]
    pub provider: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
#[serde(default)]
pub struct Subscription {
    #[prost(int32, tag = "1")]
    pub interval: i32,
    #[prost(string, tag = "2")]
    pub provider: String,
    #[prost(string, tag = "3")]
    pub query: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
#[serde(default)]
pub struct Notification {
    #[prost(string, tag = "2")]
    pub value: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
#[serde(default)]
pub struct MenuRequest {
    #[prost(string, tag = "1")]
    pub menu: String,
}

pub fn decode<T: Message + Default + DeserializeOwned>(format: u8, body: &[u8]) -> Result<T> {
    match format {
        0 => Ok(T::decode(body)?),
        1 => Ok(serde_json::from_slice(body)?),
        _ => bail!("Unsupported message encoding"),
    }
}

pub fn encode<T: Message + Serialize>(format: u8, message: &T) -> Result<Vec<u8>> {
    match format {
        0 => Ok(message.encode_to_vec()),
        1 => Ok(serde_json::to_vec(message)?),
        _ => bail!("Unsupported message encoding"),
    }
}

pub fn read_request(stream: &mut impl Read) -> Result<Option<(u8, u8, Vec<u8>)>> {
    let mut header = [0; 6];
    if stream.read(&mut header[..1])? == 0 {
        return Ok(None);
    }
    stream.read_exact(&mut header[1..])?;
    ensure!(header[0] <= STATE && header[1] <= 1, "Unsupported request");
    let size = u32::from_be_bytes(header[2..].try_into()?) as usize;
    ensure!(size <= MAX_REQUEST, "Request is too large");
    let mut body = vec![0; size];
    stream.read_exact(&mut body)?;
    Ok(Some((header[0], header[1], body)))
}

pub fn write_frame(stream: &mut impl Write, kind: u8, body: &[u8]) -> Result<()> {
    ensure!(body.len() <= 16 * 1024 * 1024, "Response is too large");
    stream.write_all(&[kind])?;
    stream.write_all(&(body.len() as u32).to_be_bytes())?;
    stream.write_all(body)?;
    Ok(())
}

pub fn read_frame(stream: &mut impl Read) -> Result<(u8, Vec<u8>)> {
    let mut header = [0; 5];
    stream.read_exact(&mut header)?;
    let size = u32::from_be_bytes(header[1..].try_into()?) as usize;
    ensure!(size <= 16 * 1024 * 1024, "Response is too large");
    let mut body = vec![0; size];
    stream.read_exact(&mut body)?;
    Ok((header[0], body))
}

pub fn write_request<T: Message + Serialize>(
    stream: &mut impl Write,
    kind: u8,
    format: u8,
    message: &T,
) -> Result<()> {
    let body = encode(format, message)?;
    ensure!(body.len() <= MAX_REQUEST, "Request is too large");
    stream.write_all(&[kind, format])?;
    stream.write_all(&(body.len() as u32).to_be_bytes())?;
    stream.write_all(&body)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_encodings_preserve_context_and_literal_values() {
        let activation = Activation {
            provider: "runner".into(),
            identifier: "printf '%s' '$HOME;literal'".into(),
            context: "opaque:workspace-context".into(),
            ..Default::default()
        };
        for format in [0, 1] {
            let mut wire = vec![];
            write_request(&mut wire, ACTIVATE, format, &activation).unwrap();
            let (kind, received, body) = read_request(&mut &wire[..]).unwrap().unwrap();
            assert_eq!(kind, ACTIVATE);
            assert_eq!(decode::<Activation>(received, &body).unwrap(), activation);
        }
    }

    #[test]
    fn malformed_and_oversized_frames_fail_before_allocating_the_body() {
        assert!(read_request(&mut &[0, 1, 255, 255, 255, 255][..]).is_err());
        assert!(read_request(&mut &[200, 1, 0, 0, 0, 0][..]).is_err());
        assert!(read_request(&mut &[0, 1, 0, 0, 0, 2, b'{'][..]).is_err());
        assert!(read_request(&mut &[][..]).unwrap().is_none());
    }
}
