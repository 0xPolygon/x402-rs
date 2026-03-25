/**
 * Amoy payment helper: builds ERC-3009 transferWithAuthorization payloads
 * and calls the facilitator's /verify and /settle endpoints directly.
 *
 * USDC on Polygon Amoy uses ERC-3009 (gasless transfer authorizations).
 * The buyer signs an EIP-712 typed message — the facilitator submits it on-chain.
 */

import { type LocalAccount, type Address, toHex } from "viem";

// Amoy USDC contract
export const AMOY_USDC_ADDRESS = "0x41E94Eb019C0762f9Bfcf9Fb1E58725BfB0e7582" as Address;
export const AMOY_CHAIN_ID = 80002n;

// EIP-712 domain for Polygon Amoy USDC
// Name is "USDC" (not "USD Coin") per the Amoy contract deployment
const USDC_DOMAIN = {
  name: "USDC",
  version: "2",
  chainId: AMOY_CHAIN_ID,
  verifyingContract: AMOY_USDC_ADDRESS,
} as const;

// ERC-3009 transferWithAuthorization typed data
const TRANSFER_WITH_AUTHORIZATION_TYPES = {
  TransferWithAuthorization: [
    { name: "from", type: "address" },
    { name: "to", type: "address" },
    { name: "value", type: "uint256" },
    { name: "validAfter", type: "uint256" },
    { name: "validBefore", type: "uint256" },
    { name: "nonce", type: "bytes32" },
  ],
} as const;

export interface PaymentOptions {
  /** Amount in raw token units (USDC has 6 decimals, so 1000 = 0.001 USDC) */
  amount: bigint;
  /** Recipient address (the "payTo" merchant address) */
  payTo: Address;
  /** Seconds from now that the authorization remains valid (default: 300) */
  maxTimeoutSeconds?: number;
  /** x402 scheme: "v1-eip155-exact" or "v2-eip155-exact" */
  scheme: "v1-eip155-exact" | "v2-eip155-exact";
  /** Optional resource URL (for v2 resource metadata) */
  resourceUrl?: string;
}

export interface PaymentRequest {
  paymentPayload: unknown;
  paymentRequirements: unknown;
}

/** Generate a cryptographically random 32-byte nonce */
function randomNonce(): `0x${string}` {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return toHex(bytes);
}

/**
 * Build a signed x402 payment request (paymentPayload + paymentRequirements)
 * for the given buyer account and options.
 */
export async function buildPaymentRequest(
  account: LocalAccount,
  opts: PaymentOptions
): Promise<PaymentRequest> {
  const maxTimeoutSeconds = opts.maxTimeoutSeconds ?? 300;
  const now = Math.floor(Date.now() / 1000);
  const validAfter = BigInt(now - 600); // 10 min in past — immediately valid
  const validBefore = BigInt(now + maxTimeoutSeconds);
  const nonce = randomNonce();

  const authorization = {
    from: account.address,
    to: opts.payTo,
    value: opts.amount,
    validAfter,
    validBefore,
    nonce,
  } as const;

  const signature = await account.signTypedData({
    domain: USDC_DOMAIN,
    types: TRANSFER_WITH_AUTHORIZATION_TYPES,
    primaryType: "TransferWithAuthorization",
    message: authorization,
  });

  // V1 uses human-readable network name; V2 uses CAIP-2
  const v1Network = "polygon-amoy";
  const v2Network = `eip155:${AMOY_CHAIN_ID}`;
  const amountStr = opts.amount.toString();

  // UnixTimestamp and TokenAmount in the Rust types deserialize from strings, not integers
  const evmPayload = {
    signature,
    authorization: {
      from: account.address,
      to: opts.payTo,
      value: amountStr,
      validAfter: validAfter.toString(),
      validBefore: validBefore.toString(),
      nonce,
    },
  };

  if (opts.scheme === "v1-eip155-exact") {
    // V1 wire format: network is human-readable name, scheme is "exact"
    const paymentPayload = {
      x402Version: 1,
      scheme: "exact",
      network: v1Network,
      payload: evmPayload,
    };
    const paymentRequirements = {
      scheme: "exact",
      network: v1Network,
      maxAmountRequired: amountStr,
      resource: opts.resourceUrl ?? "https://example.com/",
      description: "x402 test payment",
      mimeType: "application/json",
      outputSchema: null,
      payTo: opts.payTo,
      maxTimeoutSeconds,
      asset: AMOY_USDC_ADDRESS,
      extra: {
        name: "USDC",
        version: "2",
      },
    };
    return { paymentPayload, paymentRequirements };
  } else {
    // V2 wire format: network is CAIP-2, scheme is "exact"
    const paymentRequirements = {
      scheme: "exact",
      network: v2Network,
      amount: amountStr,
      payTo: opts.payTo,
      maxTimeoutSeconds,
      asset: AMOY_USDC_ADDRESS,
      extra: {
        name: "USDC",
        version: "2",
      },
    };
    const paymentPayload = {
      x402Version: 2,
      accepted: paymentRequirements,
      payload: evmPayload,
      resource: {
        description: "x402 test payment",
        mimeType: "application/json",
        url: opts.resourceUrl ?? "https://example.com/",
      },
    };
    return { paymentPayload, paymentRequirements };
  }
}

export interface VerifyResponse {
  isValid: boolean;
  invalidReason?: string;
  invalidReasonDetails?: string;
  payer?: string;
}

export interface SettleResponse {
  success: boolean;
  transaction?: string;
  network?: string;
  errorReason?: string;
  errorReasonDetails?: string;
  payer?: string;
}

function requestBody(req: PaymentRequest): string {
  // The facilitator reads x402Version from the top-level of the request body
  // to route to the correct scheme handler, so it must appear at both levels.
  const payload = req.paymentPayload as { x402Version?: number };
  return JSON.stringify({
    x402Version: payload.x402Version,
    paymentPayload: req.paymentPayload,
    paymentRequirements: req.paymentRequirements,
  });
}

/** POST to /verify on the facilitator */
export async function verifyPayment(
  facilitatorUrl: URL,
  req: PaymentRequest
): Promise<{ status: number; body: VerifyResponse }> {
  const response = await fetch(new URL("./verify", facilitatorUrl), {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: requestBody(req),
  });
  const body = await response.json() as VerifyResponse;
  return { status: response.status, body };
}

/** POST to /settle on the facilitator */
export async function settlePayment(
  facilitatorUrl: URL,
  req: PaymentRequest
): Promise<{ status: number; body: SettleResponse }> {
  const response = await fetch(new URL("./settle", facilitatorUrl), {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: requestBody(req),
  });
  const body = await response.json() as SettleResponse;
  return { status: response.status, body };
}
