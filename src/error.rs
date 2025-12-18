//! Structured error types for chain operations.
//!
//! Error types follow these principles:
//! - Each error preserves its source via `#[source]` for full error chain traversal
//! - Errors are `'static` and `Send + Sync` for cross-thread handling
//! - Location information is captured via `#[track_caller]` at error creation

use serde::Serialize;
use std::error::Error as StdError;
use std::fmt;
use std::panic::Location;
use thiserror::Error;

// ============================================================================
// Error Category
// ============================================================================

/// High-level error category for client consumption.
///
/// This enum provides a simplified view of errors for clients to determine
/// appropriate handling strategies (retry, fix payload, add funds, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    /// Network/RPC connectivity issues - retry may help
    Network,
    /// Contract execution failed - check payload/params
    Contract,
    /// Signature verification failed - re-sign payload
    Signature,
    /// Insufficient balance or funds - add funds
    Balance,
    /// Transaction processing issue - may retry
    Transaction,
    /// Invalid request format - fix payload
    Validation,
    /// Internal server error - contact support
    Internal,
}

// ============================================================================
// Error Location Tracking
// ============================================================================

/// Location information captured at error creation site.
///
/// This struct captures file, line, and column information using Rust's
/// `#[track_caller]` mechanism, enabling accurate stack trace generation
/// without runtime overhead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorLocation {
    /// Source file path where the error was created.
    pub file: &'static str,
    /// Line number in the source file.
    pub line: u32,
    /// Column number in the source file.
    pub column: u32,
}

impl ErrorLocation {
    /// Capture the current caller location.
    ///
    /// This should be called from a function marked with `#[track_caller]`
    /// to capture the location of the actual call site.
    #[track_caller]
    pub fn capture() -> Self {
        let loc = Location::caller();
        Self {
            file: loc.file(),
            line: loc.line(),
            column: loc.column(),
        }
    }

    /// Create a location for unknown/external sources.
    pub fn unknown() -> Self {
        Self {
            file: "unknown",
            line: 0,
            column: 0,
        }
    }

    /// Format as a stack frame string: "function_name\n\tfile:line"
    pub fn as_stack_frame(&self, function_name: &str) -> String {
        format!("{}\n\t{}:{}", function_name, self.file, self.line)
    }
}

impl fmt::Display for ErrorLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line, self.column)
    }
}

impl Default for ErrorLocation {
    fn default() -> Self {
        Self::unknown()
    }
}

// ============================================================================
// Error Context
// ============================================================================

/// Context for a failed operation with location tracking.
///
/// ErrorContext captures both a human-readable description of what operation
/// was being attempted and the source code location where the error occurred.
///
/// # Example
///
/// ```rust,ignore
/// use x402_rs::error::ErrorContext;
///
/// let ctx = ErrorContext::new("fetch token balance");
/// let ctx_detailed = ErrorContext::with_details(
///     "fetch token balance",
///     format!("token={}, account={}", token_addr, account_addr)
/// );
/// ```
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// Human-readable operation description.
    pub operation: &'static str,
    /// Location where the error was created.
    pub location: ErrorLocation,
    /// Additional context details (addresses, values, etc.).
    pub details: Option<String>,
}

impl ErrorContext {
    /// Create a new context with just an operation description.
    ///
    /// Location is automatically captured from the call site.
    #[track_caller]
    pub fn new(operation: &'static str) -> Self {
        Self {
            operation,
            location: ErrorLocation::capture(),
            details: None,
        }
    }

    /// Create a new context with operation description and additional details.
    ///
    /// Location is automatically captured from the call site.
    #[track_caller]
    pub fn with_details(operation: &'static str, details: impl Into<String>) -> Self {
        Self {
            operation,
            location: ErrorLocation::capture(),
            details: Some(details.into()),
        }
    }

    /// Format as a stack frame string.
    pub fn as_stack_frame(&self) -> String {
        self.location.as_stack_frame(self.operation)
    }
}

impl fmt::Display for ErrorContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.operation)?;
        if let Some(ref details) = self.details {
            write!(f, " ({})", details)?;
        }
        Ok(())
    }
}

// ============================================================================
// EVM-Specific Errors
// ============================================================================

/// EVM chain errors with preserved source chains and location tracking.
///
/// Each variant captures:
/// - A context describing the operation that failed
/// - The underlying source error (where applicable)
/// - Location information for stack trace generation
#[derive(Debug, Error)]
pub enum EvmError {
    /// RPC transport layer failure (connection, timeout, etc.).
    #[error("{context}: RPC transport failed")]
    Transport {
        context: ErrorContext,
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },

    /// Contract call reverted or failed.
    #[error("{context}: contract call failed")]
    ContractCall {
        context: ErrorContext,
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },

    /// Transaction send failed.
    #[error("{context}: transaction send failed")]
    TransactionSend {
        context: ErrorContext,
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },

    /// Transaction receipt fetch failed or timed out.
    #[error("{context}: receipt fetch failed")]
    ReceiptFetch {
        context: ErrorContext,
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },

    /// Gas price estimation failed.
    #[error("{context}: gas price fetch failed")]
    GasPrice {
        context: ErrorContext,
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },

    /// Balance query failed.
    #[error("{context}: balance query failed")]
    BalanceQuery {
        context: ErrorContext,
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },

    /// EIP-6492 or ECDSA signature decode/verification failed.
    #[error("{context}: signature processing failed")]
    SignatureProcessing {
        context: ErrorContext,
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },

    /// Contract code existence check failed.
    #[error("{context}: contract code check failed")]
    CodeCheck {
        context: ErrorContext,
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },

    /// EIP-712 domain resolution failed.
    #[error("{context}: EIP-712 domain resolution failed")]
    DomainResolution {
        context: ErrorContext,
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },

    /// Nonce management error.
    #[error("{context}: nonce management failed")]
    NonceManagement {
        context: ErrorContext,
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
}

impl EvmError {
    /// Get the location where this error was created.
    pub fn location(&self) -> &ErrorLocation {
        match self {
            Self::Transport { context, .. } => &context.location,
            Self::ContractCall { context, .. } => &context.location,
            Self::TransactionSend { context, .. } => &context.location,
            Self::ReceiptFetch { context, .. } => &context.location,
            Self::GasPrice { context, .. } => &context.location,
            Self::BalanceQuery { context, .. } => &context.location,
            Self::SignatureProcessing { context, .. } => &context.location,
            Self::CodeCheck { context, .. } => &context.location,
            Self::DomainResolution { context, .. } => &context.location,
            Self::NonceManagement { context, .. } => &context.location,
        }
    }

    /// Get the context for this error.
    pub fn context(&self) -> &ErrorContext {
        match self {
            Self::Transport { context, .. } => context,
            Self::ContractCall { context, .. } => context,
            Self::TransactionSend { context, .. } => context,
            Self::ReceiptFetch { context, .. } => context,
            Self::GasPrice { context, .. } => context,
            Self::BalanceQuery { context, .. } => context,
            Self::SignatureProcessing { context, .. } => context,
            Self::CodeCheck { context, .. } => context,
            Self::DomainResolution { context, .. } => context,
            Self::NonceManagement { context, .. } => context,
        }
    }

    /// Get the error variant name.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Transport { .. } => "EvmError::Transport",
            Self::ContractCall { .. } => "EvmError::ContractCall",
            Self::TransactionSend { .. } => "EvmError::TransactionSend",
            Self::ReceiptFetch { .. } => "EvmError::ReceiptFetch",
            Self::GasPrice { .. } => "EvmError::GasPrice",
            Self::BalanceQuery { .. } => "EvmError::BalanceQuery",
            Self::SignatureProcessing { .. } => "EvmError::SignatureProcessing",
            Self::CodeCheck { .. } => "EvmError::CodeCheck",
            Self::DomainResolution { .. } => "EvmError::DomainResolution",
            Self::NonceManagement { .. } => "EvmError::NonceManagement",
        }
    }

    /// Returns true if this error is transient and the operation may succeed on retry.
    ///
    /// Transient errors include network issues, gas price fluctuations, and nonce
    /// management problems. Non-transient errors include signature failures,
    /// contract reverts, and balance insufficiency.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Transport { .. }
                | Self::GasPrice { .. }
                | Self::ReceiptFetch { .. }
                | Self::NonceManagement { .. }
        )
    }

    /// Returns the high-level error category for client handling.
    pub fn category(&self) -> ErrorCategory {
        match self {
            Self::Transport { .. } | Self::GasPrice { .. } => ErrorCategory::Network,
            Self::ContractCall { .. } | Self::CodeCheck { .. } => ErrorCategory::Contract,
            Self::SignatureProcessing { .. } | Self::DomainResolution { .. } => {
                ErrorCategory::Signature
            }
            Self::BalanceQuery { .. } => ErrorCategory::Balance,
            Self::TransactionSend { .. }
            | Self::ReceiptFetch { .. }
            | Self::NonceManagement { .. } => ErrorCategory::Transaction,
        }
    }

    /// Returns a machine-readable error code for client consumption.
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::Transport { .. } => "EVM_TRANSPORT_ERROR",
            Self::ContractCall { .. } => "EVM_CONTRACT_CALL_ERROR",
            Self::TransactionSend { .. } => "EVM_TRANSACTION_SEND_ERROR",
            Self::ReceiptFetch { .. } => "EVM_RECEIPT_FETCH_ERROR",
            Self::GasPrice { .. } => "EVM_GAS_PRICE_ERROR",
            Self::BalanceQuery { .. } => "EVM_BALANCE_QUERY_ERROR",
            Self::SignatureProcessing { .. } => "EVM_SIGNATURE_ERROR",
            Self::CodeCheck { .. } => "EVM_CODE_CHECK_ERROR",
            Self::DomainResolution { .. } => "EVM_DOMAIN_RESOLUTION_ERROR",
            Self::NonceManagement { .. } => "EVM_NONCE_ERROR",
        }
    }
}

// ============================================================================
// Solana-Specific Errors
// ============================================================================

/// Solana chain errors with preserved source chains and location tracking.
#[derive(Debug, Error)]
pub enum SolanaError {
    /// RPC call to Solana cluster failed.
    #[error("{context}: RPC call failed")]
    Rpc {
        context: ErrorContext,
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },

    /// Transaction simulation failed.
    #[error("{context}: transaction simulation failed")]
    Simulation {
        context: ErrorContext,
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },

    /// Transaction signing failed.
    #[error("{context}: transaction signing failed")]
    Signing {
        context: ErrorContext,
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },

    /// Transaction confirmation timed out.
    #[error("{context}: transaction confirmation timeout")]
    ConfirmationTimeout {
        context: ErrorContext,
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },

    /// Instruction decode/validation failed.
    #[error("{context}: instruction validation failed - {reason}")]
    InstructionValidation {
        context: ErrorContext,
        reason: String,
    },

    /// Account lookup or validation failed.
    #[error("{context}: account lookup failed")]
    AccountLookup {
        context: ErrorContext,
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },

    /// Transaction send failed.
    #[error("{context}: transaction send failed")]
    TransactionSend {
        context: ErrorContext,
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
}

impl SolanaError {
    /// Get the location where this error was created.
    pub fn location(&self) -> &ErrorLocation {
        match self {
            Self::Rpc { context, .. } => &context.location,
            Self::Simulation { context, .. } => &context.location,
            Self::Signing { context, .. } => &context.location,
            Self::ConfirmationTimeout { context, .. } => &context.location,
            Self::InstructionValidation { context, .. } => &context.location,
            Self::AccountLookup { context, .. } => &context.location,
            Self::TransactionSend { context, .. } => &context.location,
        }
    }

    /// Get the context for this error.
    pub fn context(&self) -> &ErrorContext {
        match self {
            Self::Rpc { context, .. } => context,
            Self::Simulation { context, .. } => context,
            Self::Signing { context, .. } => context,
            Self::ConfirmationTimeout { context, .. } => context,
            Self::InstructionValidation { context, .. } => context,
            Self::AccountLookup { context, .. } => context,
            Self::TransactionSend { context, .. } => context,
        }
    }

    /// Get the error variant name.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Rpc { .. } => "SolanaError::Rpc",
            Self::Simulation { .. } => "SolanaError::Simulation",
            Self::Signing { .. } => "SolanaError::Signing",
            Self::ConfirmationTimeout { .. } => "SolanaError::ConfirmationTimeout",
            Self::InstructionValidation { .. } => "SolanaError::InstructionValidation",
            Self::AccountLookup { .. } => "SolanaError::AccountLookup",
            Self::TransactionSend { .. } => "SolanaError::TransactionSend",
        }
    }

    /// Returns true if this error is transient and the operation may succeed on retry.
    ///
    /// Transient errors include RPC connectivity issues, confirmation timeouts,
    /// and transaction send failures (which may be due to network congestion).
    /// Non-transient errors include signing failures, simulation failures,
    /// instruction validation errors, and account lookup failures.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Rpc { .. } | Self::ConfirmationTimeout { .. } | Self::TransactionSend { .. }
        )
    }

    /// Returns the high-level error category for client handling.
    pub fn category(&self) -> ErrorCategory {
        match self {
            Self::Rpc { .. } => ErrorCategory::Network,
            Self::Simulation { .. } | Self::InstructionValidation { .. } => ErrorCategory::Contract,
            Self::Signing { .. } => ErrorCategory::Signature,
            Self::AccountLookup { .. } => ErrorCategory::Balance,
            Self::TransactionSend { .. } | Self::ConfirmationTimeout { .. } => {
                ErrorCategory::Transaction
            }
        }
    }

    /// Returns a machine-readable error code for client consumption.
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::Rpc { .. } => "SOLANA_RPC_ERROR",
            Self::Simulation { .. } => "SOLANA_SIMULATION_ERROR",
            Self::Signing { .. } => "SOLANA_SIGNING_ERROR",
            Self::ConfirmationTimeout { .. } => "SOLANA_CONFIRMATION_TIMEOUT",
            Self::InstructionValidation { .. } => "SOLANA_INSTRUCTION_VALIDATION_ERROR",
            Self::AccountLookup { .. } => "SOLANA_ACCOUNT_LOOKUP_ERROR",
            Self::TransactionSend { .. } => "SOLANA_TRANSACTION_SEND_ERROR",
        }
    }
}

// ============================================================================
// Unified Chain Error
// ============================================================================

/// Unified chain error wrapping EVM and Solana errors.
///
/// This enum provides a single type for chain-specific errors that can be
/// used in the higher-level facilitator errors.
#[derive(Debug, Error)]
pub enum ChainError {
    /// EVM-specific error.
    #[error(transparent)]
    Evm(#[from] EvmError),

    /// Solana-specific error.
    #[error(transparent)]
    Solana(#[from] SolanaError),
}

impl ChainError {
    /// Get the location where this error was created.
    pub fn location(&self) -> &ErrorLocation {
        match self {
            Self::Evm(e) => e.location(),
            Self::Solana(e) => e.location(),
        }
    }

    /// Get the error variant name.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Evm(e) => e.kind_name(),
            Self::Solana(e) => e.kind_name(),
        }
    }

    /// Returns true if this error is transient and the operation may succeed on retry.
    ///
    /// Delegates to the underlying chain-specific error's `is_transient()` method.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Evm(e) => e.is_transient(),
            Self::Solana(e) => e.is_transient(),
        }
    }

    /// Returns the high-level error category for client handling.
    ///
    /// Delegates to the underlying chain-specific error's `category()` method.
    pub fn category(&self) -> ErrorCategory {
        match self {
            Self::Evm(e) => e.category(),
            Self::Solana(e) => e.category(),
        }
    }

    /// Returns a machine-readable error code for client consumption.
    ///
    /// Delegates to the underlying chain-specific error's `error_code()` method.
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::Evm(e) => e.error_code(),
            Self::Solana(e) => e.error_code(),
        }
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn test_error_location_capture() {
        let loc = ErrorLocation::capture();
        assert!(loc.file.ends_with("error.rs"));
        assert!(loc.line > 0);
    }

    #[test]
    fn test_error_location_unknown() {
        let loc = ErrorLocation::unknown();
        assert_eq!(loc.file, "unknown");
        assert_eq!(loc.line, 0);
        assert_eq!(loc.column, 0);
    }

    #[test]
    fn test_error_location_as_stack_frame() {
        let loc = ErrorLocation {
            file: "/app/src/main.rs",
            line: 42,
            column: 5,
        };
        let frame = loc.as_stack_frame("my_function");
        assert_eq!(frame, "my_function\n\t/app/src/main.rs:42");
    }

    #[test]
    fn test_error_context_new() {
        let ctx = ErrorContext::new("fetch balance");
        assert_eq!(ctx.operation, "fetch balance");
        assert!(ctx.details.is_none());
        assert!(ctx.location.file.ends_with("error.rs"));
    }

    #[test]
    fn test_error_context_with_details() {
        let ctx = ErrorContext::with_details("fetch balance", "token=0x123");
        assert_eq!(ctx.operation, "fetch balance");
        assert_eq!(ctx.details.as_deref(), Some("token=0x123"));
    }

    #[test]
    fn test_error_context_display() {
        let ctx = ErrorContext::with_details("fetch balance", "token=0x123");
        assert_eq!(format!("{}", ctx), "fetch balance (token=0x123)");

        let ctx_no_details = ErrorContext::new("simple operation");
        assert_eq!(format!("{}", ctx_no_details), "simple operation");
    }

    #[test]
    fn test_evm_error_creation_and_chain() {
        let io_err = io::Error::new(io::ErrorKind::ConnectionRefused, "connection refused");

        let evm_err = EvmError::Transport {
            context: ErrorContext::with_details("fetch balance", "token=USDC"),
            source: Box::new(io_err),
        };

        assert_eq!(evm_err.kind_name(), "EvmError::Transport");
        assert!(evm_err.location().file.ends_with("error.rs"));

        // Test that the error message contains expected text
        let msg = format!("{}", evm_err);
        assert!(msg.contains("RPC transport failed"));

        let chain_err = ChainError::Evm(evm_err);
        assert!(chain_err.kind_name().starts_with("EvmError"));

        // ChainError is transparent, so its Display shows the inner error
        let chain_msg = format!("{}", chain_err);
        assert!(chain_msg.contains("RPC transport failed"));
    }

    #[test]
    fn test_solana_error_creation() {
        let sol_err = SolanaError::InstructionValidation {
            context: ErrorContext::new("verify transfer instruction"),
            reason: "invalid program id".to_string(),
        };

        assert_eq!(sol_err.kind_name(), "SolanaError::InstructionValidation");
        let msg = format!("{}", sol_err);
        assert!(msg.contains("invalid program id"));
    }

    #[test]
    fn test_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<EvmError>();
        assert_send_sync::<SolanaError>();
        assert_send_sync::<ChainError>();
    }

    #[test]
    fn test_error_is_static() {
        fn assert_static<T: 'static>() {}
        assert_static::<EvmError>();
        assert_static::<SolanaError>();
        assert_static::<ChainError>();
    }
}
