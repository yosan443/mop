use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: String,
    pub ts: DateTime<Utc>,
    pub user_id: Option<String>,
    pub username: Option<String>,
    pub action: String,
    pub resource_kind: Option<String>,
    pub resource_id: Option<String>,
    pub detail_json: Option<String>,
    pub result: AuditResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditResult {
    Success,
    Failure,
    Denied,
}

impl AuditResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuditResult::Success => "success",
            AuditResult::Failure => "failure",
            AuditResult::Denied => "denied",
        }
    }
}
