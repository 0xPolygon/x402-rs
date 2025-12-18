use std::time::SystemTimeError;

use crate::chain::evm::EvmProvider;
use crate::chain::solana::SolanaProvider;
use crate::error::{ChainError, ErrorCategory, EvmError, SolanaError};
use crate::facilitator::Facilitator;
use crate::network::{Network, NetworkFamily};
use crate::types::{
    MixedAddress, Scheme, SettleRequest, SettleResponse, SupportedPaymentKindsResponse,
    VerifyRequest, VerifyResponse,
};

pub mod evm;
pub mod solana;

pub enum NetworkProvider {
    Evm(EvmProvider),
    Solana(SolanaProvider),
}

pub trait FromEnvByNetworkBuild: Sized {
    fn from_env(
        network: Network,
    ) -> impl Future<Output = Result<Option<Self>, Box<dyn std::error::Error>>> + Send;
}

impl FromEnvByNetworkBuild for NetworkProvider {
    async fn from_env(network: Network) -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let family: NetworkFamily = network.into();
        let provider = match family {
            NetworkFamily::Evm => {
                let provider = EvmProvider::from_env(network).await?;
                provider.map(NetworkProvider::Evm)
            }
            NetworkFamily::Solana => {
                let provider = SolanaProvider::from_env(network).await?;
                provider.map(NetworkProvider::Solana)
            }
        };
        Ok(provider)
    }
}

pub trait NetworkProviderOps {
    fn signer_address(&self) -> MixedAddress;
    fn network(&self) -> Network;
}

impl NetworkProviderOps for NetworkProvider {
    fn signer_address(&self) -> MixedAddress {
        match self {
            NetworkProvider::Evm(provider) => provider.signer_address(),
            NetworkProvider::Solana(provider) => provider.signer_address(),
        }
    }

    fn network(&self) -> Network {
        match self {
            NetworkProvider::Evm(provider) => provider.network(),
            NetworkProvider::Solana(provider) => provider.network(),
        }
    }
}

impl Facilitator for NetworkProvider {
    type Error = FacilitatorLocalError;

    async fn verify(&self, request: &VerifyRequest) -> Result<VerifyResponse, Self::Error> {
        match self {
            NetworkProvider::Evm(provider) => provider.verify(request).await,
            NetworkProvider::Solana(provider) => provider.verify(request).await,
        }
    }

    async fn settle(&self, request: &SettleRequest) -> Result<SettleResponse, Self::Error> {
        match self {
            NetworkProvider::Evm(provider) => provider.settle(request).await,
            NetworkProvider::Solana(provider) => provider.settle(request).await,
        }
    }

    async fn supported(&self) -> Result<SupportedPaymentKindsResponse, Self::Error> {
        match self {
            NetworkProvider::Evm(provider) => provider.supported().await,
            NetworkProvider::Solana(provider) => provider.supported().await,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FacilitatorLocalError {
    /// The network is not supported by this facilitator.
    #[error("Unsupported network")]
    UnsupportedNetwork(Option<MixedAddress>),
    /// The network is not supported by this facilitator.
    #[error("Network mismatch: expected {1}, actual {2}")]
    NetworkMismatch(Option<MixedAddress>, Network, Network),
    /// Scheme mismatch.
    #[error("Scheme mismatch: expected {1}, actual {2}")]
    SchemeMismatch(Option<MixedAddress>, Scheme, Scheme),
    /// Invalid address.
    #[error("Invalid address: {0}")]
    InvalidAddress(String),
    /// The `pay_to` recipient in the requirements doesn't match the `to` address in the payload.
    #[error("Incompatible payload receivers (payload: {1}, requirements: {2})")]
    ReceiverMismatch(MixedAddress, String, String),
    /// Failed to read a system clock to check timing.
    #[error("Can not get system clock")]
    ClockError(#[source] SystemTimeError),
    /// The `validAfter`/`validBefore` fields on the authorization are not within bounds.
    #[error("Invalid timing: {1}")]
    InvalidTiming(MixedAddress, String),
    /// Low-level contract interaction failure (e.g. call failed, method not found).
    /// DEPRECATED: Use `Evm` or `Solana` variants with proper error types.
    #[error("Invalid contract call: {0}")]
    ContractCall(String),
    /// EVM chain error with full source chain and location tracking.
    #[error("EVM error")]
    Evm(#[source] EvmError),
    /// Solana chain error with full source chain and location tracking.
    #[error("Solana error")]
    Solana(#[source] SolanaError),
    /// EIP-712 signature is invalid or mismatched.
    #[error("Invalid signature: {1}")]
    InvalidSignature(MixedAddress, String),
    /// The payer's on-chain balance is insufficient for the payment.
    #[error("Insufficient funds")]
    InsufficientFunds(MixedAddress),
    /// The payload's `value` is not enough to meet the requirements.
    #[error("Insufficient value")]
    InsufficientValue(MixedAddress),
    /// The payload decoding failed.
    #[error("Decoding error: {0}")]
    DecodingError(String),
}

impl From<EvmError> for FacilitatorLocalError {
    fn from(err: EvmError) -> Self {
        FacilitatorLocalError::Evm(err)
    }
}

impl From<SolanaError> for FacilitatorLocalError {
    fn from(err: SolanaError) -> Self {
        FacilitatorLocalError::Solana(err)
    }
}

impl From<ChainError> for FacilitatorLocalError {
    fn from(err: ChainError) -> Self {
        match err {
            ChainError::Evm(e) => FacilitatorLocalError::Evm(e),
            ChainError::Solana(e) => FacilitatorLocalError::Solana(e),
        }
    }
}

impl FacilitatorLocalError {
    /// Returns true if this error is transient and the operation may succeed on retry.
    ///
    /// Transient errors include:
    /// - EVM/Solana chain errors that are themselves transient (network issues, timeouts)
    /// - System clock errors (temporary system resource issues)
    ///
    /// Non-transient errors include:
    /// - Validation errors (invalid address, scheme mismatch, etc.)
    /// - Signature errors (invalid signature)
    /// - Insufficient funds/value (requires user action)
    /// - Decoding errors (invalid payload format)
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Evm(e) => e.is_transient(),
            Self::Solana(e) => e.is_transient(),
            Self::ClockError(_) => true, // System clock issues are transient
            // All other errors are non-transient
            Self::UnsupportedNetwork(_)
            | Self::NetworkMismatch(..)
            | Self::SchemeMismatch(..)
            | Self::InvalidAddress(_)
            | Self::ReceiverMismatch(..)
            | Self::InvalidTiming(..)
            | Self::ContractCall(_)
            | Self::InvalidSignature(..)
            | Self::InsufficientFunds(_)
            | Self::InsufficientValue(_)
            | Self::DecodingError(_) => false,
        }
    }

    /// Returns the high-level error category for client handling.
    ///
    /// Categories help clients determine appropriate handling strategies:
    /// - `Network`: Retry may help
    /// - `Contract`: Check payload/params
    /// - `Signature`: Re-sign payload
    /// - `Balance`: Add funds
    /// - `Transaction`: May retry
    /// - `Validation`: Fix payload format
    /// - `Internal`: Contact support
    pub fn category(&self) -> ErrorCategory {
        match self {
            Self::Evm(e) => e.category(),
            Self::Solana(e) => e.category(),
            Self::InvalidSignature(..) => ErrorCategory::Signature,
            Self::InsufficientFunds(_) | Self::InsufficientValue(_) => ErrorCategory::Balance,
            Self::UnsupportedNetwork(_)
            | Self::NetworkMismatch(..)
            | Self::SchemeMismatch(..)
            | Self::InvalidAddress(_)
            | Self::ReceiverMismatch(..)
            | Self::InvalidTiming(..)
            | Self::DecodingError(_) => ErrorCategory::Validation,
            Self::ContractCall(_) => ErrorCategory::Contract,
            Self::ClockError(_) => ErrorCategory::Internal,
        }
    }

    /// Returns a machine-readable error code for client consumption.
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::Evm(e) => e.error_code(),
            Self::Solana(e) => e.error_code(),
            Self::UnsupportedNetwork(_) => "UNSUPPORTED_NETWORK",
            Self::NetworkMismatch(..) => "NETWORK_MISMATCH",
            Self::SchemeMismatch(..) => "SCHEME_MISMATCH",
            Self::InvalidAddress(_) => "INVALID_ADDRESS",
            Self::ReceiverMismatch(..) => "RECEIVER_MISMATCH",
            Self::ClockError(_) => "CLOCK_ERROR",
            Self::InvalidTiming(..) => "INVALID_TIMING",
            Self::ContractCall(_) => "CONTRACT_CALL_ERROR",
            Self::InvalidSignature(..) => "INVALID_SIGNATURE",
            Self::InsufficientFunds(_) => "INSUFFICIENT_FUNDS",
            Self::InsufficientValue(_) => "INSUFFICIENT_VALUE",
            Self::DecodingError(_) => "DECODING_ERROR",
        }
    }
}
