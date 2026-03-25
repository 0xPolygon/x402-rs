use alloy_primitives::B256;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use url::Url;
use x402_types::chain::ChainId;
use x402_types::config::LiteralOrEnv;

use crate::chain::Eip155ChainReference;

/// Configuration for an EVM-compatible chain in the x402 facilitator.
///
/// This struct combines a chain reference with chain-specific configuration
/// including RPC endpoints, signers, and network capabilities.
///
/// # Example
///
/// ```ignore
/// use x402_chain_eip155::chain::{Eip155ChainConfig, Eip155ChainReference};
///
/// let config = Eip155ChainConfig {
///     chain_reference: Eip155ChainReference::new(8453), // Base
///     inner: config_inner,
/// };
/// ```
#[derive(Debug, Clone)]
pub struct Eip155ChainConfig {
    /// The numeric chain ID for this EVM network.
    pub chain_reference: Eip155ChainReference,
    /// Chain-specific configuration details.
    pub inner: Eip155ChainConfigInner,
}

impl Eip155ChainConfig {
    /// Returns the CAIP-2 chain ID for this configuration.
    pub fn chain_id(&self) -> ChainId {
        self.chain_reference.into()
    }
    /// Returns whether this chain supports EIP-1559 gas pricing.
    pub fn eip1559(&self) -> bool {
        self.inner.eip1559
    }

    /// Returns whether this chain supports flashblocks (immediate block finality).
    pub fn flashblocks(&self) -> bool {
        self.inner.flashblocks
    }

    /// Returns the transaction receipt timeout in seconds.
    pub fn receipt_timeout_secs(&self) -> u64 {
        self.inner.receipt_timeout_secs
    }

    /// Returns the signer configuration for this chain.
    pub fn signers(&self) -> &Eip155SignersConfig {
        &self.inner.signers
    }

    /// Returns the RPC endpoint configurations for this chain.
    pub fn rpc(&self) -> &Vec<RpcConfig> {
        &self.inner.rpc
    }

    /// Returns the numeric chain reference.
    pub fn chain_reference(&self) -> Eip155ChainReference {
        self.chain_reference
    }
}

/// Configuration specific to EVM-compatible chains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eip155ChainConfigInner {
    /// Whether the chain supports EIP-1559 gas pricing.
    #[serde(default = "eip155_chain_config::default_eip1559")]
    pub eip1559: bool,
    /// Whether the chain supports flashblocks.
    #[serde(default = "eip155_chain_config::default_flashblocks")]
    pub flashblocks: bool,
    /// Signer configuration for this chain (required).
    /// Accepts either:
    /// - An array of private keys or env var references: `["$KEY_1", "$KEY_2"]`
    /// - A single env var containing comma-separated keys: `"$EVM_PRIVATE_KEY"`
    #[serde(deserialize_with = "deserialize_signers")]
    pub signers: Eip155SignersConfig,
    /// RPC provider configuration for this chain (required).
    pub rpc: Vec<RpcConfig>,
    /// How long to wait till the transaction receipt is available (optional)
    #[serde(default = "eip155_chain_config::default_receipt_timeout_secs")]
    pub receipt_timeout_secs: u64,
}

mod eip155_chain_config {
    pub fn default_eip1559() -> bool {
        true
    }
    pub fn default_flashblocks() -> bool {
        false
    }
    pub fn default_receipt_timeout_secs() -> u64 {
        30
    }
}

/// RPC provider configuration for a single provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcConfig {
    /// HTTP URL for the RPC endpoint.
    /// Supports env var references: `"$RPC_URL"` or `"${RPC_URL}"`
    pub http: LiteralOrEnv<Url>,
    /// Rate limit for requests per second (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<u32>,
}

/// Configuration for EVM signers.
///
/// Deserializes an array of private key strings (hex format, 0x-prefixed) and
/// validates them as valid 32-byte private keys. The `EthereumWallet` is created
/// lazily when needed via the `wallet()` method.
///
/// Each string can be:
/// - A literal hex private key: `"0xcafe..."`
/// - An environment variable reference: `"$PRIVATE_KEY"` or `"${PRIVATE_KEY}"`
///
/// Example JSON:
/// ```json
/// {
///   "signers": [
///     "$HOT_WALLET_KEY",
///     "0xcafe000000000000000000000000000000000000000000000000000000000001"
///   ]
/// }
/// ```
pub type Eip155SignersConfig = Vec<LiteralOrEnv<EvmPrivateKey>>;

// ============================================================================
// EVM Private Key
// ============================================================================

/// A validated EVM private key (32 bytes).
///
/// This type represents a raw private key that has been validated as a proper
/// 32-byte hex value. It can be converted to a `PrivateKeySigner` when needed.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct EvmPrivateKey(B256);

impl EvmPrivateKey {
    /// Get the raw 32 bytes of the private key.
    pub fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_ref()
    }
}

impl PartialEq for EvmPrivateKey {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl FromStr for EvmPrivateKey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        B256::from_str(s)
            .map(Self)
            .map_err(|e| format!("Invalid evm private key: {}", e))
    }
}

/// Custom deserializer for signers that supports both formats:
/// - Array of strings: `["$KEY_1", "$KEY_2"]` (each resolved via LiteralOrEnv)
/// - Single string: `"$EVM_PRIVATE_KEY"` (resolved, then split by comma)
fn deserialize_signers<'de, D>(deserializer: D) -> Result<Eip155SignersConfig, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct SignersVisitor;

    impl<'de> de::Visitor<'de> for SignersVisitor {
        type Value = Eip155SignersConfig;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str(
                "an array of private keys or a single string with comma-separated keys",
            )
        }

        // Array format: ["$KEY_1", "$KEY_2", "0xdead..."]
        fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            Vec::<LiteralOrEnv<EvmPrivateKey>>::deserialize(
                de::value::SeqAccessDeserializer::new(seq),
            )
        }

        // String format: "$EVM_PRIVATE_KEY" → resolve env → split by comma
        fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            // Resolve env var if present
            let resolved = if let Some(var_name) = parse_env_var_name(s) {
                std::env::var(&var_name).map_err(|_| {
                    de::Error::custom(format!(
                        "Environment variable '{}' not found (referenced as '{}')",
                        var_name, s
                    ))
                })?
            } else {
                s.to_string()
            };

            // Split by comma and parse each key
            let keys: Result<Vec<_>, _> = resolved
                .split(',')
                .map(|k| k.trim())
                .filter(|k| !k.is_empty())
                .map(|k| {
                    k.parse::<EvmPrivateKey>()
                        .map(LiteralOrEnv::from_literal)
                        .map_err(|e| de::Error::custom(e))
                })
                .collect();

            keys
        }
    }

    deserializer.deserialize_any(SignersVisitor)
}

/// Parse `$VAR` or `${VAR}` syntax, returning the variable name.
fn parse_env_var_name(s: &str) -> Option<String> {
    if s.starts_with("${") && s.ends_with('}') {
        Some(s[2..s.len() - 1].to_string())
    } else if s.starts_with('$') && s.len() > 1 {
        let var_name = &s[1..];
        if var_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            Some(var_name.to_string())
        } else {
            None
        }
    } else {
        None
    }
}
