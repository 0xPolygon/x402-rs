//! HTTP endpoints implemented by the x402 **facilitator**.
//!
//! These are the server-side handlers for processing client-submitted x402 payments.
//! They include both protocol-critical endpoints (`/verify`, `/settle`) and discovery endpoints (`/supported`, etc).
//!
//! All payloads follow the types defined in the `x402-rs` crate, and are compatible
//! with the TypeScript and Go client SDKs.
//!
//! Each endpoint consumes or produces structured JSON payloads defined in `x402-rs`,
//! and is compatible with official x402 client SDKs.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router, response::IntoResponse};
use serde_json::json;
use tracing::instrument;

use std::fmt::Debug;

use crate::chain::FacilitatorLocalError;
use crate::facilitator::Facilitator;
use crate::telemetry::{RequestContext, RequestContextGuard};
use crate::types::{
    ErrorDetails, FacilitatorErrorReason, MixedAddress, SettleRequest, StructuredErrorResponse,
    VerifyRequest, VerifyResponse,
};

/// `GET /verify`: Returns a machine-readable description of the `/verify` endpoint.
///
/// This is served by the facilitator to help clients understand how to construct
/// a valid [`VerifyRequest`] for payment verification.
///
/// This is optional metadata and primarily useful for discoverability and debugging tools.
#[instrument(skip_all)]
pub async fn get_verify_info() -> impl IntoResponse {
    Json(json!({
        "endpoint": "/verify",
        "description": "POST to verify x402 payments",
        "body": {
            "paymentPayload": "PaymentPayload",
            "paymentRequirements": "PaymentRequirements",
        }
    }))
}

/// `GET /settle`: Returns a machine-readable description of the `/settle` endpoint.
///
/// This is served by the facilitator to describe the structure of a valid
/// [`SettleRequest`] used to initiate on-chain payment settlement.
#[instrument(skip_all)]
pub async fn get_settle_info() -> impl IntoResponse {
    Json(json!({
        "endpoint": "/settle",
        "description": "POST to settle x402 payments",
        "body": {
            "paymentPayload": "PaymentPayload",
            "paymentRequirements": "PaymentRequirements",
        }
    }))
}

pub fn routes<A>() -> Router<A>
where
    A: Facilitator + Clone + Send + Sync + 'static,
    A::Error: IntoResponse + std::error::Error,
{
    Router::new()
        .route("/", get(get_root))
        .route("/verify", get(get_verify_info))
        .route("/verify", post(post_verify::<A>))
        .route("/settle", get(get_settle_info))
        .route("/settle", post(post_settle::<A>))
        .route("/health", get(get_health::<A>))
        .route("/supported", get(get_supported::<A>))
}

/// `GET /`: Returns a simple greeting message from the facilitator.
#[instrument(skip_all)]
pub async fn get_root() -> impl IntoResponse {
    let pkg_name = env!("CARGO_PKG_NAME");
    (StatusCode::OK, format!("Hello from {pkg_name}!"))
}

/// `GET /supported`: Lists the x402 payment schemes and networks supported by this facilitator.
///
/// Facilitators may expose this to help clients dynamically configure their payment requests
/// based on available network and scheme support.
#[instrument(skip_all)]
pub async fn get_supported<A>(State(facilitator): State<A>) -> impl IntoResponse
where
    A: Facilitator,
    A::Error: IntoResponse,
{
    match facilitator.supported().await {
        Ok(supported) => (StatusCode::OK, Json(json!(supported))).into_response(),
        Err(error) => error.into_response(),
    }
}

#[instrument(skip_all)]
pub async fn get_health<A>(State(facilitator): State<A>) -> impl IntoResponse
where
    A: Facilitator,
    A::Error: IntoResponse,
{
    get_supported(State(facilitator)).await
}

/// `POST /verify`: Facilitator-side verification of a proposed x402 payment.
///
/// This endpoint checks whether a given payment payload satisfies the declared
/// [`PaymentRequirements`], including signature validity, scheme match, and fund sufficiency.
///
/// Responds with a [`VerifyResponse`] indicating whether the payment can be accepted.
#[instrument(skip_all)]
pub async fn post_verify<A>(
    State(facilitator): State<A>,
    Json(body): Json<VerifyRequest>,
) -> impl IntoResponse
where
    A: Facilitator,
    A::Error: IntoResponse + std::error::Error,
{
    let request_id = crate::telemetry::get_request_id();

    // Extract key business data for logging
    let network = body.payment_payload.network.to_string();
    let scheme = body.payment_payload.scheme.to_string();

    // Set request context for downstream chain operations
    RequestContext::set(RequestContext {
        request_id: request_id.clone(),
        network: Some(network.clone()),
        payer: None,
        operation: "verify",
    });
    let _guard = RequestContextGuard; // Clears context when handler returns

    tracing::info!(
        event = "verify_start",
        request_id = %request_id,
        network = %network,
        scheme = %scheme,
        "processing verify request"
    );

    match facilitator.verify(&body).await {
        Ok(valid_response) => {
            let (is_valid, payer) = match &valid_response {
                VerifyResponse::Valid { payer } => (true, Some(payer.to_string())),
                VerifyResponse::Invalid { payer, .. } => (false, payer.as_ref().map(|p| p.to_string())),
            };
            tracing::info!(
                event = "verify_completed",
                request_id = %request_id,
                network = %network,
                payer = ?payer,
                valid = %is_valid,
                "payment verification completed"
            );
            (StatusCode::OK, Json(valid_response)).into_response()
        }
        Err(error) => {
            // Log with DataDog-compatible error format
            log_error_datadog(
                "verify_error",
                &request_id,
                &error,
                Some(&network),
                None,
            );
            error.into_response()
        }
    }
}

/// `POST /settle`: Facilitator-side execution of a valid x402 payment on-chain.
///
/// Given a valid [`SettleRequest`], this endpoint attempts to execute the payment
/// via ERC-3009 `transferWithAuthorization`, and returns a [`SettleResponse`] with transaction details.
///
/// This endpoint is typically called after a successful `/verify` step.
#[instrument(skip_all)]
pub async fn post_settle<A>(
    State(facilitator): State<A>,
    Json(body): Json<SettleRequest>,
) -> impl IntoResponse
where
    A: Facilitator,
    A::Error: IntoResponse + std::error::Error,
{
    let request_id = crate::telemetry::get_request_id();

    // Extract key business data for logging
    let network = body.payment_payload.network.to_string();
    let scheme = body.payment_payload.scheme.to_string();
    let (payer, payee, amount) = match &body.payment_payload.payload {
        crate::types::ExactPaymentPayload::Evm(evm) => (
            Some(evm.authorization.from.to_string()),
            Some(evm.authorization.to.to_string()),
            Some(evm.authorization.value.to_string()),
        ),
        crate::types::ExactPaymentPayload::Solana(_) => (None, None, None),
    };

    // Set request context for downstream chain operations
    RequestContext::set(RequestContext {
        request_id: request_id.clone(),
        network: Some(network.clone()),
        payer: payer.clone(),
        operation: "settle",
    });
    let _guard = RequestContextGuard; // Clears context when handler returns

    tracing::info!(
        event = "settle_start",
        request_id = %request_id,
        payer = ?payer,
        payee = ?payee,
        network = %network,
        scheme = %scheme,
        amount = ?amount,
        "processing settle request"
    );

    match facilitator.settle(&body).await {
        Ok(valid_response) => {
            tracing::info!(
                event = "settle_success",
                request_id = %request_id,
                payer = ?payer,
                payee = ?payee,
                network = %network,
                amount = ?amount,
                transaction = ?valid_response.transaction,
                success = %valid_response.success,
                "payment settlement completed"
            );
            (StatusCode::OK, Json(valid_response)).into_response()
        }
        Err(error) => {
            // Log with DataDog-compatible error format
            log_error_datadog(
                "settle_error",
                &request_id,
                &error,
                Some(&network),
                payer.as_deref(),
            );
            error.into_response()
        }
    }
}

fn invalid_schema(payer: Option<MixedAddress>) -> VerifyResponse {
    VerifyResponse::invalid(payer, FacilitatorErrorReason::InvalidScheme)
}

/// Log an error with DataDog-compatible error format.
///
/// This logs errors in a format that DataDog Error Tracking can parse,
/// including `error.kind`, `error.message`, and `error.stack` fields.
///
/// Works with any error type that implements `Debug` and `std::error::Error`.
fn log_error_datadog<E: Debug + std::error::Error>(
    event: &str,
    request_id: &str,
    error: &E,
    network: Option<&str>,
    payer: Option<&str>,
) {
    // Build error kind from type name
    let error_kind = std::any::type_name::<E>();

    // Build error message with full chain
    let error_message = build_generic_error_chain(error);

    // Build stack trace from error chain
    let error_stack = build_generic_error_stack(error);

    tracing::error!(
        event = %event,
        request_id = %request_id,
        network = ?network,
        payer = ?payer,
        error.kind = %error_kind,
        error.message = %error_message,
        error.stack = %error_stack,
        "{}",
        error_message
    );
}

/// Build a complete error message by traversing the error chain.
fn build_generic_error_chain<E: std::error::Error>(error: &E) -> String {
    let mut messages = vec![error.to_string()];
    let mut current: &dyn std::error::Error = error;

    while let Some(source) = current.source() {
        messages.push(source.to_string());
        current = source;
    }

    messages.join(": ")
}

/// Build a stack trace from the error chain for DataDog Error Tracking.
///
/// DataDog requires at least 2 lines with 1 meaningful frame for error tracking.
fn build_generic_error_stack<E: std::error::Error>(error: &E) -> String {
    let mut frames = Vec::new();

    // Add frame for the top-level error
    frames.push(format!(
        "  at {} (facilitator)",
        std::any::type_name::<E>()
    ));

    // Add frames for each error in the chain
    let mut current: &dyn std::error::Error = error;
    let mut depth = 1;

    while let Some(source) = current.source() {
        let frame = format!(
            "  at caused_by[{}]: {} (chain)",
            depth,
            truncate_message(&source.to_string(), 100)
        );
        frames.push(frame);
        current = source;
        depth += 1;
    }

    // Ensure at least 2 lines for DataDog
    if frames.len() < 2 {
        frames.push("  at <no additional context>".to_string());
    }

    format!("Error: {}\n{}", error, frames.join("\n"))
}

/// Truncate a message to a maximum length, adding ellipsis if needed.
fn truncate_message(msg: &str, max_len: usize) -> String {
    if msg.len() <= max_len {
        msg.to_string()
    } else {
        format!("{}...", &msg[..max_len - 3])
    }
}

impl IntoResponse for FacilitatorLocalError {
    fn into_response(self) -> Response {
        let error = self;

        // Helper to build a structured error response
        let make_structured_error = |err: &FacilitatorLocalError| -> StructuredErrorResponse {
            let is_transient = err.is_transient();
            StructuredErrorResponse {
                error: ErrorDetails {
                    code: err.error_code().to_string(),
                    message: err.to_string(),
                    category: format!("{:?}", err.category()).to_lowercase(),
                    transient: is_transient,
                    retry_after_ms: if is_transient { Some(1000) } else { None },
                },
                request_id: None, // Request ID not available in IntoResponse context
            }
        };

        match error {
            FacilitatorLocalError::SchemeMismatch(payer, ..) => {
                (StatusCode::OK, Json(invalid_schema(payer))).into_response()
            }
            FacilitatorLocalError::ReceiverMismatch(payer, ..)
            | FacilitatorLocalError::InvalidSignature(payer, ..)
            | FacilitatorLocalError::InvalidTiming(payer, ..)
            | FacilitatorLocalError::InsufficientValue(payer) => {
                (StatusCode::OK, Json(invalid_schema(Some(payer)))).into_response()
            }
            FacilitatorLocalError::NetworkMismatch(payer, ..)
            | FacilitatorLocalError::UnsupportedNetwork(payer) => (
                StatusCode::OK,
                Json(VerifyResponse::invalid(
                    payer,
                    FacilitatorErrorReason::InvalidNetwork,
                )),
            )
                .into_response(),
            FacilitatorLocalError::DecodingError(reason) => (
                StatusCode::OK,
                Json(VerifyResponse::invalid(
                    None,
                    FacilitatorErrorReason::FreeForm(reason),
                )),
            )
                .into_response(),
            FacilitatorLocalError::InsufficientFunds(payer) => (
                StatusCode::OK,
                Json(VerifyResponse::invalid(
                    Some(payer),
                    FacilitatorErrorReason::InsufficientFunds,
                )),
            )
                .into_response(),
            // Chain errors and internal errors return structured error responses
            ref err @ FacilitatorLocalError::Evm(_)
            | ref err @ FacilitatorLocalError::Solana(_) => {
                // Use 502 Bad Gateway for upstream chain errors
                (StatusCode::BAD_GATEWAY, Json(make_structured_error(err))).into_response()
            }
            ref err @ FacilitatorLocalError::ContractCall(..)
            | ref err @ FacilitatorLocalError::InvalidAddress(..) => {
                (StatusCode::BAD_REQUEST, Json(make_structured_error(err))).into_response()
            }
            ref err @ FacilitatorLocalError::ClockError(_) => {
                // Internal server error for clock issues
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(make_structured_error(err)),
                )
                    .into_response()
            }
        }
    }
}
