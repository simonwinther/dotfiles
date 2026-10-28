use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fmt::Write, io};
use unicode_general_category::{GeneralCategory, get_general_category};

pub const MAIN_PAGE: &str = "https://chatgpt.com";
pub const EXTENSION_ORIGIN: &str = "chrome-extension://lfiijailpfkonkfmfkcjfagnopohffge/";
pub const MAX_MESSAGE: usize = 4 * 1024 * 1024;
pub const MAX_PROJECTS: usize = 10_000;
const MAX_TEXT: usize = 1024;

pub(crate) fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub fn clean(text: &str) -> String {
    text.chars()
        .map(|c| match get_general_category(c) {
            GeneralCategory::Control
            | GeneralCategory::Format
            | GeneralCategory::PrivateUse
            | GeneralCategory::Surrogate
            | GeneralCategory::Unassigned => ' ',
            _ => c,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn valid_text(text: &str) -> bool {
    !text.trim().is_empty() && text.chars().count() <= MAX_TEXT
}

fn valid_id(id: &str) -> bool {
    id.strip_prefix("g-p-").is_some_and(|value| {
        value.len() == 32
            && value
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    })
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub url: String,
}

impl Project {
    pub fn home() -> Self {
        Self {
            id: "home".into(),
            name: "Open main page".into(),
            url: MAIN_PAGE.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Snapshot {
    pub version: u32,
    pub complete: bool,
    pub account_id: String,
    pub workspace_id: String,
    pub workspace_name: String,
    pub started_at: u64,
    pub projects: Vec<Project>,
}

impl Snapshot {
    pub fn validate(&self, now: u64) -> io::Result<()> {
        if self.version != 1 || !self.complete {
            return Err(invalid("Expected a complete version 1 snapshot"));
        }
        if ![&self.account_id, &self.workspace_id, &self.workspace_name]
            .into_iter()
            .all(|s| valid_text(s))
        {
            return Err(invalid("Invalid account or workspace"));
        }
        if self.started_at == 0 || self.started_at > now.saturating_add(60_000) {
            return Err(invalid("Invalid snapshot time"));
        }
        if self.projects.len() > MAX_PROJECTS {
            return Err(invalid("Invalid project list"));
        }
        let mut seen = HashSet::new();
        for project in &self.projects {
            if !valid_id(&project.id) || !seen.insert(&project.id) {
                return Err(invalid("Invalid or duplicate project ID"));
            }
            if !valid_text(&project.name) || clean(&project.name).is_empty() {
                return Err(invalid("Invalid project name"));
            }
            if project.url != format!("{MAIN_PAGE}/g/{}/project", project.id) {
                return Err(invalid("Invalid project URL"));
            }
        }
        Ok(())
    }

    pub fn sort(&mut self) {
        self.projects
            .sort_by_cached_key(|p| (clean(&p.name).to_lowercase(), p.id.clone()));
    }

    pub fn context_key(&self) -> String {
        // Match Python's json.dumps([account_id, workspace_id]), including its
        // spaces and ensure_ascii escapes, so existing version-1 keys survive.
        fn ascii_json(text: &str) -> String {
            let json = serde_json::to_string(text).expect("strings serialize");
            let mut result = String::new();
            for c in json.chars() {
                if (' '..='~').contains(&c) {
                    result.push(c);
                } else {
                    for unit in c.encode_utf16(&mut [0; 2]) {
                        write!(result, "\\u{unit:04x}").expect("write to string");
                    }
                }
            }
            result
        }
        let json = format!(
            "[{}, {}]",
            ascii_json(&self.account_id),
            ascii_json(&self.workspace_id)
        );
        format!("{:x}", Sha256::digest(json.as_bytes()))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StoredSnapshot {
    #[serde(flatten)]
    pub snapshot: Snapshot,
    pub synced_at: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Status {
    pub code: String,
    pub at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

fn valid_code(code: &str) -> bool {
    matches!(
        code,
        "syncing"
            | "auth_required"
            | "workspace_unknown"
            | "invalid_response"
            | "pagination"
            | "account_changed"
            | "network"
            | "timeout"
            | "unexpected"
            | "ok"
    ) || code.strip_prefix("http_").is_some_and(|http| {
        let bytes = http.as_bytes();
        bytes.len() == 3
            && (b'1'..=b'5').contains(&bytes[0])
            && bytes[1..].iter().all(u8::is_ascii_digit)
    })
}

impl Status {
    pub fn from_message(value: &Value, now: u64) -> io::Result<Self> {
        let code = value["code"]
            .as_str()
            .filter(|code| valid_code(code))
            .ok_or_else(|| invalid("Unknown sync status"))?;
        Ok(Self {
            code: code.into(),
            at: now,
            stage: value["stage"]
                .as_str()
                .filter(|s| matches!(*s, "session" | "workspace" | "projects"))
                .map(str::to_owned),
            kind: value["kind"]
                .as_str()
                .filter(|s| matches!(*s, "TypeError" | "SyntaxError" | "TimeoutError" | "Error"))
                .map(str::to_owned),
        })
    }

    pub fn notice(&self, since: u64, now: u64) -> Option<String> {
        if self.at <= since || self.code == "ok" || !valid_code(&self.code) {
            return None;
        }
        if self.code == "syncing" && now.saturating_sub(self.at) > 120_000 {
            return Some("Sync interrupted · Refresh the website to retry".into());
        }
        if let Some(http) = self.code.strip_prefix("http_") {
            return Some(format!(
                "Sync failed · Website request returned HTTP {http}"
            ));
        }
        let message = match self.code.as_str() {
            "syncing" => "Syncing…",
            "auth_required" => "Sign in to sync projects",
            "workspace_unknown" => "Workspace lookup failed",
            "network" => "Offline · showing saved projects",
            "timeout" => "Sync timed out · showing saved projects",
            "invalid_response" => "Unsupported project list · showing saved projects",
            "pagination" => "Sync failed · Project pagination did not finish",
            "account_changed" => "Account changed · waiting for sync",
            _ => "Sync failed · showing saved projects",
        };
        let details: Vec<_> = [self.stage.as_deref(), self.kind.as_deref()]
            .into_iter()
            .flatten()
            .map(clean)
            .collect();
        Some(if details.is_empty() {
            message.into()
        } else {
            format!("{message} · {}", details.join(" / "))
        })
    }
}
