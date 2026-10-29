use thiserror::Error;

pub type Result<T> = std::result::Result<T, FlintError>;

#[derive(Debug, Error)]
pub enum FlintError {
    #[error("кошелёк не создан")]
    NotInitialized,

    #[error("кошелёк заблокирован")]
    Locked,

    #[error("нет такого slate: {0}")]
    UnknownSlate(String),

    #[error("неверные параметры: {0}")]
    Invalid(String),

    #[error("узел недоступен: {0}")]
    Node(String),

    #[error("ошибка кошелька: {0}")]
    Wallet(String),

    /// Cooperative embedded scan stopped at a batch boundary.
    #[error("скан отменён пользователем")]
    ScanCancelled,

    #[error("ошибка ввода-вывода: {0}")]
    Io(#[from] std::io::Error),

    #[error("ошибка формата: {0}")]
    Json(#[from] serde_json::Error),
}

impl FlintError {
    pub fn wallet(e: impl std::fmt::Display) -> Self {
        FlintError::Wallet(e.to_string())
    }

    pub fn node(e: impl std::fmt::Display) -> Self {
        FlintError::Node(e.to_string())
    }
}

/// Ошибки, после которых нельзя повторять receive как новую операцию. У patched
/// fork недоступный/повреждённый recovery тоже означает fail-closed, хотя сам
/// response мог сохраниться только в upstream.
pub(crate) fn receive_tx_already_mutated(error: &str) -> bool {
    [
        "TransactionAlreadyReceived",
        "ReceiveRecoveryUnavailable",
        "ReceiveRecoveryIntegrity",
    ]
    .iter()
    .any(|code| error.contains(code))
}

#[cfg(test)]
mod tests {
    use super::receive_tx_already_mutated;

    #[test]
    fn only_mutation_or_recovery_failures_forbid_receive_retry() {
        for error in [
            "foreign receive_tx: TransactionAlreadyReceived",
            "ReceiveRecoveryUnavailable: slate-id",
            "ReceiveRecoveryIntegrity: запрос не совпадает",
        ] {
            assert!(receive_tx_already_mutated(error), "{error}");
        }
        for error in [
            "DB Not Found",
            "Slate id",
            "connection reset",
            "invalid slate",
            "timeout",
            "already received",
            "ReceiveRecovery",
        ] {
            assert!(!receive_tx_already_mutated(error), "{error}");
        }
    }
}
