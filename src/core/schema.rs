use serde::{Deserialize, Serialize};

/// Inline ADR attached to a FatProposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdrProposal {
    pub title: String,
    pub reason: String,
}

/// Full agent transaction: a state patch plus optional ADR and human message.
///
/// Used as the MCP input schema for `propose_state_update`.
/// All optional fields use `#[serde(default)]` for backward compatibility.
#[derive(Debug, Serialize, Deserialize)]
pub struct FatProposal {
    pub patch: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adr_proposal: Option<AdrProposal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_to_human: Option<String>,
}
