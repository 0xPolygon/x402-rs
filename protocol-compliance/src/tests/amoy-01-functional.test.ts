/**
 * Amoy Functional Tests
 *
 * Tests the full happy-path and error cases against a real Polygon Amoy facilitator.
 * One facilitator process is spawned for the entire file.
 */

import { describe, it, expect, beforeAll, afterAll } from "vitest";
import { RSFacilitatorHandle } from "../utils/facilitator.js";
import { makeAmoyFacilitatorConfig } from "../utils/amoy-config.js";
import { WalletPool } from "../utils/amoy-wallets.js";
import {
  buildPaymentRequest,
  verifyPayment,
  settlePayment,
  AMOY_USDC_ADDRESS,
  type PaymentOptions,
} from "../utils/amoy-payment.js";

const PAYMENT_AMOUNT = 1000n; // 0.001 USDC (6 decimals)
const PAY_TO = "0xc50f9F01159BeA6901F3F8dd97C28c7A1663369D" as const;

let facilitator: RSFacilitatorHandle;
const walletPool = new WalletPool();

function buyerOpts(scheme: PaymentOptions["scheme"]): PaymentOptions {
  return {
    amount: PAYMENT_AMOUNT,
    payTo: PAY_TO,
    scheme,
    maxTimeoutSeconds: 300,
  };
}

beforeAll(async () => {
  const configPath = await makeAmoyFacilitatorConfig();
  facilitator = await RSFacilitatorHandle.spawn(undefined, configPath);
}, 120_000);

afterAll(async () => {
  await facilitator.stop();
});

// ============================================================================
// Health & Discovery
// ============================================================================

describe("Health & Discovery", () => {
  it("GET /health → 200", async () => {
    const res = await fetch(new URL("./health", facilitator.url));
    expect(res.status).toBe(200);
  });

  it("GET /supported → includes Amoy (80002) with exact scheme in v1 and v2", async () => {
    const res = await fetch(new URL("./supported", facilitator.url));
    expect(res.status).toBe(200);
    const body = await res.json() as { kinds: { x402Version: number; scheme: string; network: string }[] };
    // V1 uses human-readable network name; V2 uses CAIP-2
    const hasV1 = body.kinds.some((k) => k.x402Version === 1 && k.scheme === "exact" && k.network === "polygon-amoy");
    const hasV2 = body.kinds.some((k) => k.x402Version === 2 && k.scheme === "exact" && k.network === "eip155:80002");
    expect(hasV1, `v1 kind not found in: ${JSON.stringify(body.kinds)}`).toBe(true);
    expect(hasV2, `v2 kind not found in: ${JSON.stringify(body.kinds)}`).toBe(true);
  });

  it("GET /verify → 200 (schema info)", async () => {
    const res = await fetch(new URL("./verify", facilitator.url));
    expect(res.status).toBe(200);
  });

  it("GET /settle → 200 (schema info)", async () => {
    const res = await fetch(new URL("./settle", facilitator.url));
    expect(res.status).toBe(200);
  });
});

// ============================================================================
// V1 Exact Scheme — happy path
// ============================================================================

describe("V1 Exact Scheme — happy path", () => {
  it("POST /verify with valid v1 payment → { isValid: true }", async () => {
    const { account } = walletPool.next();
    const req = await buildPaymentRequest(account, buyerOpts("v1-eip155-exact"));
    const { status, body } = await verifyPayment(facilitator.url, req);
    expect(status).toBe(200);
    expect(body.isValid).toBe(true);
  }, 30_000);

  it("POST /settle after valid v1 verify → { success: true, transaction: '0x...' }", async () => {
    const { account } = walletPool.next();
    const req = await buildPaymentRequest(account, buyerOpts("v1-eip155-exact"));
    // Verify first
    const verifyResult = await verifyPayment(facilitator.url, req);
    expect(verifyResult.body.isValid).toBe(true);
    // Then settle
    const { status, body } = await settlePayment(facilitator.url, req);
    console.log(`[v1 settle after verify] tx: ${body.transaction} https://amoy.polygonscan.com/tx/${body.transaction}`);
    expect(status).toBe(200);
    expect(body.success).toBe(true);
    expect(body.transaction).toMatch(/^0x/);
  }, 60_000);

  it("POST /settle direct (no prior verify) → facilitator re-verifies, returns success", async () => {
    const { account } = walletPool.next();
    const req = await buildPaymentRequest(account, buyerOpts("v1-eip155-exact"));
    const { status, body } = await settlePayment(facilitator.url, req);
    console.log(`[v1 settle direct] tx: ${body.transaction} https://amoy.polygonscan.com/tx/${body.transaction}`);
    expect(status).toBe(200);
    expect(body.success).toBe(true);
  }, 60_000);
});

// ============================================================================
// V1 Exact Scheme — error cases
// ============================================================================

describe("V1 Exact Scheme — error cases", () => {
  it("POST /verify with expired authorization → { isValid: false }", async () => {
    const { account } = walletPool.next();
    const req = await buildPaymentRequest(account, {
      ...buyerOpts("v1-eip155-exact"),
      maxTimeoutSeconds: -1, // validBefore is in the past
    });
    // Hack: override validBefore to be in the past
    const payload = req.paymentPayload as any;
    payload.payload.authorization.validBefore = Math.floor(Date.now() / 1000) - 10;
    // Re-sign is not possible without private key re-exposure, so sign a fresh one
    // Instead, manually craft an expired auth by setting validBefore to 1 (far past)
    const expiredReq = await buildPaymentRequest(account, buyerOpts("v1-eip155-exact"));
    (expiredReq.paymentPayload as any).payload.authorization.validBefore = 1;

    const { status, body } = await verifyPayment(facilitator.url, expiredReq);
    // Facilitator should reject with 400 and isValid: false
    expect(status).toBe(400);
    expect(body.isValid).toBe(false);
  }, 30_000);

  it("POST /verify with wrong amount → { isValid: false }", async () => {
    const { account } = walletPool.next();
    const req = await buildPaymentRequest(account, buyerOpts("v1-eip155-exact"));
    // Claim we're paying 1 USDC but requirements say we need 1000 USDC
    (req.paymentRequirements as any).maxAmountRequired = "1000000000"; // 1000 USDC
    const { status, body } = await verifyPayment(facilitator.url, req);
    expect(status).toBe(400);
    expect(body.isValid).toBe(false);
  }, 30_000);

  it("POST /verify with wrong recipient → { isValid: false }", async () => {
    const { account } = walletPool.next();
    const req = await buildPaymentRequest(account, buyerOpts("v1-eip155-exact"));
    // Override payTo in requirements to a different address
    (req.paymentRequirements as any).payTo = "0x0000000000000000000000000000000000000002";
    const { status, body } = await verifyPayment(facilitator.url, req);
    expect(status).toBe(400);
    expect(body.isValid).toBe(false);
  }, 30_000);
});

// ============================================================================
// V2 Exact Scheme — happy path
// ============================================================================

describe("V2 Exact Scheme — happy path", () => {
  it("POST /verify with valid v2 payment → { isValid: true }", async () => {
    const { account } = walletPool.next();
    const req = await buildPaymentRequest(account, buyerOpts("v2-eip155-exact"));
    const { status, body } = await verifyPayment(facilitator.url, req);
    expect(status).toBe(200);
    expect(body.isValid).toBe(true);
  }, 30_000);

  it("POST /settle after valid v2 verify → { success: true }", async () => {
    const { account } = walletPool.next();
    const req = await buildPaymentRequest(account, buyerOpts("v2-eip155-exact"));
    const verifyResult = await verifyPayment(facilitator.url, req);
    expect(verifyResult.body.isValid).toBe(true);
    const { status, body } = await settlePayment(facilitator.url, req);
    console.log(`[v2 settle after verify] tx: ${body.transaction} https://amoy.polygonscan.com/tx/${body.transaction}`);
    expect(status).toBe(200);
    expect(body.success).toBe(true);
    expect(body.transaction).toMatch(/^0x/);
  }, 60_000);
});

// ============================================================================
// Multi-wallet round-robin
// ============================================================================

describe("Multi-wallet round-robin", () => {
  it("5 sequential payments from 5 different buyer wallets all succeed", async () => {
    const pool = new WalletPool();
    for (let i = 0; i < 5; i++) {
      const { account } = pool.next();
      const req = await buildPaymentRequest(account, buyerOpts("v1-eip155-exact"));
      const { status, body } = await settlePayment(facilitator.url, req);
      console.log(`[multi-wallet wallet ${i}] tx: ${body.transaction} https://amoy.polygonscan.com/tx/${body.transaction}`);
      expect(status, `wallet ${i} failed`).toBe(200);
      expect(body.success, `wallet ${i} success false`).toBe(true);
    }
  }, 300_000);
});
