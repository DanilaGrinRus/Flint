//! Platform-neutral transfer contract shared by desktop and the iOS FFI.
//!
//! This crate contains only typed money/stage/action rules. It has no UI,
//! filesystem, network, Windows, or secret-storage code.

use serde::{Deserialize, Serialize};

pub const SCHEMA: u32 = 1;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Network {
    Testnet,
    Mainnet,
}

impl Network {
    pub fn parse(value: &str) -> Result<Self, ContractError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "testnet" | "test" | "floonet" => Ok(Self::Testnet),
            "mainnet" | "main" => Ok(Self::Mainnet),
            _ => Err(ContractError::Network(value.to_string())),
        }
    }

    pub fn hrp(self) -> &'static str {
        match self {
            Self::Testnet => "tgrin",
            Self::Mainnet => "grin",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Send,
    Receive,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Send => "send",
            Self::Receive => "receive",
        }
    }
}

/// Amount in nanogrins, never a floating-point value.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
pub struct NanoGrin(pub u64);

impl NanoGrin {
    pub fn parse(value: &str) -> Result<Self, ContractError> {
        let value = value.trim();
        if value.is_empty() {
            return Err(ContractError::Amount("пустая сумма".into()));
        }
        let (whole, frac) = match value.split_once('.') {
            Some((whole, frac)) => (whole, frac),
            None => (value, ""),
        };
        if frac.len() > 9 || !whole.bytes().all(|b| b.is_ascii_digit()) {
            return Err(ContractError::Amount(value.to_string()));
        }
        let whole: u64 = whole
            .parse()
            .map_err(|_| ContractError::Amount(value.to_string()))?;
        let mut frac = frac.to_string();
        if !frac.is_empty() {
            frac.extend(std::iter::repeat('0').take(9 - frac.len()));
        } else {
            frac.push_str("000000000");
        }
        let nano: u64 = frac
            .parse()
            .map_err(|_| ContractError::Amount(value.to_string()))?;
        whole
            .checked_mul(1_000_000_000)
            .and_then(|v| v.checked_add(nano))
            .map(Self)
            .ok_or(ContractError::Amount(value.to_string()))
    }

    pub fn checked_add(self, other: Self) -> Result<Self, ContractError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(ContractError::Amount("переполнение".into()))
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, ContractError> {
        self.0
            .checked_sub(other.0)
            .map(Self)
            .ok_or(ContractError::Amount("недостаточно средств".into()))
    }

    pub fn display(self) -> String {
        format!("{}.{:09}", self.0 / 1_000_000_000, self.0 % 1_000_000_000)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Prepared,
    RequestSent,
    ResponseReady,
    Finalized,
    Posted,
    Confirmed,
    Cancelled,
    Failed,
}

impl Stage {
    /// Map the existing wallet journal state without losing its backend-neutral
    /// meaning. Unknown states are recoverable rather than treated as confirmed.
    pub fn from_wallet_state(kind: &str, state: &str) -> Self {
        match (kind, state) {
            ("receive", "response_received") => Self::ResponseReady,
            ("send", "prepared" | "locked") => Self::Prepared,
            ("send", "finalizing" | "finalized_not_posted") => Self::Finalized,
            ("send", "posted") => Self::Posted,
            ("send" | "receive", "confirmed") => Self::Confirmed,
            ("send" | "receive", "canceled" | "cancelled") => Self::Cancelled,
            ("send" | "receive", "failed_recoverable") => Self::Failed,
            _ => Self::Prepared,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Confirmed | Self::Cancelled)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Resend,
    ExportResponse,
    Finalize,
    Repost,
    Wait,
    Cancel,
    RestoreScan,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NextAction {
    Resend,
    ExportResponse,
    Finalize,
    Repost,
    Wait,
    Cancel,
    RestoreScan,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferAction {
    Resend,
    ExportResponse,
    Finalize,
    Repost,
    Wait,
    Cancel,
    RestoreScan,
}

impl Stage {
    pub fn allowed(self, action: Action) -> bool {
        match self {
            Self::Prepared => matches!(action, Action::Resend | Action::Cancel),
            Self::RequestSent => matches!(action, Action::Resend | Action::Wait),
            Self::ResponseReady => matches!(action, Action::ExportResponse | Action::Finalize),
            Self::Finalized => matches!(action, Action::Repost | Action::Wait),
            Self::Posted => matches!(action, Action::Wait | Action::RestoreScan),
            Self::Confirmed | Self::Cancelled => false,
            Self::Failed => matches!(action, Action::RestoreScan | Action::Cancel),
        }
    }

    /// The UI action is derived from the machine state, never from a claim that
    /// delivery equals network confirmation.
    pub fn next_action(self, direction: Direction) -> NextAction {
        match (self, direction) {
            (Self::Prepared, _) => NextAction::Resend,
            (Self::RequestSent, _) => NextAction::Wait,
            (Self::ResponseReady, Direction::Receive) => NextAction::ExportResponse,
            (Self::ResponseReady, _) => NextAction::Finalize,
            (Self::Finalized, _) => NextAction::Repost,
            (Self::Posted, _) => NextAction::Wait,
            (Self::Failed, _) => NextAction::RestoreScan,
            (Self::Confirmed | Self::Cancelled, _) => NextAction::Wait,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferState {
    pub schema: u32,
    pub slate_id: String,
    pub direction: Direction,
    pub stage: Stage,
    pub amount_nano: NanoGrin,
    pub fee_nano: NanoGrin,
    pub peer: String,
    pub next_action: NextAction,
    pub can_resend: bool,
    pub can_export_response: bool,
    pub can_finalize: bool,
    pub can_repost: bool,
    pub can_cancel: bool,
    pub last_error: Option<String>,
    pub updated_at: i64,
}

impl TransferState {
    pub fn new(
        slate_id: impl Into<String>,
        direction: Direction,
        stage: Stage,
        amount_nano: NanoGrin,
        fee_nano: NanoGrin,
        peer: impl Into<String>,
        last_error: Option<String>,
        updated_at: i64,
    ) -> Self {
        let next_action = stage.next_action(direction);
        Self {
            schema: SCHEMA,
            slate_id: slate_id.into(),
            direction,
            stage,
            amount_nano,
            fee_nano,
            peer: peer.into(),
            next_action,
            can_resend: stage.allowed(Action::Resend),
            can_export_response: stage.allowed(Action::ExportResponse),
            can_finalize: stage.allowed(Action::Finalize),
            can_repost: stage.allowed(Action::Repost),
            can_cancel: stage.allowed(Action::Cancel),
            last_error,
            updated_at,
        }
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema != SCHEMA {
            return Err(ContractError::Schema(self.schema));
        }
        if self.slate_id.trim().is_empty() {
            return Err(ContractError::Identity("пустой slate_id".into()));
        }
        if self.stage == Stage::Confirmed && self.next_action != NextAction::Wait {
            return Err(ContractError::Transition("confirmed должен ждать".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractError {
    Amount(String),
    Network(String),
    Schema(u32),
    Identity(String),
    Transition(String),
}

impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ContractError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_are_exact_and_checked() {
        assert_eq!(NanoGrin::parse("1.5").unwrap(), NanoGrin(1_500_000_000));
        assert_eq!(NanoGrin::parse("0.000000001").unwrap(), NanoGrin(1));
        assert!(NanoGrin::parse("-1").is_err());
        assert!(NanoGrin::parse("1.0000000001").is_err());
    }

    #[test]
    fn delivery_is_not_confirmation() {
        assert_eq!(
            Stage::from_wallet_state("send", "posted").next_action(Direction::Send),
            NextAction::Wait
        );
        assert!(!Stage::Posted.allowed(Action::Cancel));
        assert!(Stage::ResponseReady.allowed(Action::ExportResponse));
        assert!(!Stage::Prepared.allowed(Action::Finalize));
    }

    #[test]
    fn fixture_round_trips_without_private_material() {
        let state = TransferState::new(
            "00000000-0000-0000-0000-000000000001",
            Direction::Receive,
            Stage::ResponseReady,
            NanoGrin(100_000_000),
            NanoGrin(23_000_000),
            "tgrin1fixture",
            None,
            0,
        );
        state.validate().unwrap();
        let json = serde_json::to_string(&state).unwrap();
        assert!(!json.contains("password"));
        assert!(!json.contains("seed"));
        let decoded: TransferState = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, state);
    }
}
