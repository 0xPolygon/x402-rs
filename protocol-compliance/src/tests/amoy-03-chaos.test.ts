/**
 * Amoy Chaos Tests
 *
 * Resilience and edge case testing: bad inputs, timing, replay attacks,
 * concurrent nonce stress, and post-burst health checks.
 *
 * NOTE: The replay attack test costs ~0.001 USDC (real on-chain settlement).
 */

import { describe, it, expect, beforeAll, afterAll } from "vitest";
import { RSFacilitatorHandle } from "../utils/facilitator.js";
import { makeAmoyFacilitatorConfig } from "../utils/amoy-config.js";
import { WalletPool } from "../utils/amoy-wallets.js";
import {
  buildPaymentRequest,
  verifyPayment,
  settlePayment,
  type PaymentOptions,
} from "../utils/amoy-payment.js";

const PAYMENT_AMOUNT = 1000n; // 0.001 USDC
const PAY_TO = "0x0000000000000000000000000000000000000001" as const;

let facilitator: RSFacilitatorHandle;
const walletPool = new WalletPool();

function opts(overrides: Partial<PaymentOptions> = {}): PaymentOptions {
  return {
    amount: PAYMENT_AMOUNT,
    payTo: PAY_TO,
    scheme: "v1-eip155-exact",
    ...overrides,
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
// Bad inputs
// ============================================================================

describe("Bad inputs", () => {
  it("POST /verify with malformed JSON → 400 or 422", async () => {
    const res = await fetch(new URL("./verify", facilitator.url), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{ this is not valid json",
    });
    expect([400, 422]).toContain(res.status);
  });

  it("POST /verify with empty body → 400 or 422", async () => {
    const res = await fetch(new URL("./verify", facilitator.url), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{}",
    });
    expect([400, 422]).toContain(res.status);
  });

  it("POST /verify with unknown scheme → 400", async () => {
    const { account } = walletPool.next();
    const req = await buildPaymentRequest(account, opts());
    (req.paymentPayload as any).scheme = "totally-unknown-scheme";
    (req.paymentRequirements as any).scheme = "totally-unknown-scheme";
    const { status, body } = await verifyPayment(facilitator.url, req);
    expect(status).toBe(400);
    expect(body.isValid).toBe(false);
  }, 30_000);

  it("POST /verify with wrong chain ID → 400", async () => {
    const { account } = walletPool.next();
    const req = await buildPaymentRequest(account, opts());
    // Change to a non-configured chain
    (req.paymentPayload as any).network = "eip155:1"; // mainnet, not configured
    (req.paymentRequirements as any).network = "eip155:1";
    const { status, body } = await verifyPayment(facilitator.url, req);
    expect(status).toBe(400);
    expect(body.isValid).toBe(false);
  }, 30_000);

  it("POST /verify with body missing required fields → 400", async () => {
    const res = await fetch(new URL("./verify", facilitator.url), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ paymentPayload: null, paymentRequirements: null }),
    });
    expect([400, 422]).toContain(res.status);
  });
});

// ============================================================================
// Timing edge cases
// ============================================================================

describe("Timing edge cases", () => {
  it("validAfter in the future → { isValid: false }", async () => {
    const { account } = walletPool.next();
    const req = await buildPaymentRequest(account, opts());
    // Override validAfter to be 1 hour from now — authorization not yet valid
    const futureTimestamp = Math.floor(Date.now() / 1000) + 3600;
    (req.paymentPayload as any).payload.authorization.validAfter = futureTimestamp;
    const { status, body } = await verifyPayment(facilitator.url, req);
    expect(status).toBe(400);
    expect(body.isValid).toBe(false);
  }, 30_000);

  it("very short validity window (5s) — verify + settle race", async () => {
    const { account } = walletPool.next();
    const req = await buildPaymentRequest(account, opts({ maxTimeoutSeconds: 5 }));
    // Verify should still pass (just signed)
    const verifyResult = await verifyPayment(facilitator.url, req);
    // Verify may or may not succeed depending on network latency — either is valid behavior
    // If verify passes, we don't attempt settle (too risky to waste gas on a likely timeout)
    console.log(`Short-window verify: isValid=${verifyResult.body.isValid}`);
    // Just assert no crash (any 200 or 400 response is acceptable)
    expect([200, 400]).toContain(verifyResult.status);
  }, 30_000);
});

// ============================================================================
// Real replay attack (costs ~0.001 USDC)
// ============================================================================

describe("Replay protection (on-chain)", () => {
  it("same authorization settled twice → first succeeds, second fails", async () => {
    const { account } = walletPool.next();
    const req = await buildPaymentRequest(account, opts());

    // First settlement — should succeed
    const first = await settlePayment(facilitator.url, req);
    expect(first.status).toBe(200);
    expect(first.body.success).toBe(true);
    console.log(`First settle tx: ${first.body.transaction}`);

    // Second settlement with identical authorization — nonce already used on-chain
    const second = await settlePayment(facilitator.url, req);
    console.log(`Second settle status: ${second.status}, success: ${second.body.success}`);
    // Should fail — either 400 (verify rejection) or 500 (on-chain failure)
    expect(second.body.success).toBe(false);
  }, 120_000);
});

// ============================================================================
// Concurrent nonce stress
// ============================================================================

describe("Concurrent nonce stress", () => {
  it("5 simultaneous settle requests from same wallet — at least 1 succeeds, no crashes", async () => {
    // All 5 requests use the same buyer wallet with different nonces
    const { account } = walletPool.next();
    const requests = await Promise.all(
      Array.from({ length: 5 }, () => buildPaymentRequest(account, opts()))
    );

    const results = await Promise.allSettled(
      requests.map((req) => settlePayment(facilitator.url, req))
    );

    const fulfilled = results.filter((r) => r.status === "fulfilled") as
      PromiseFulfilledResult<Awaited<ReturnType<typeof settlePayment>>>[];
    const successes = fulfilled.filter((r) => r.value.body.success);

    console.log(`Concurrent nonce stress: ${successes.length}/5 settlements succeeded`);

    // At least 1 must succeed
    expect(successes.length).toBeGreaterThanOrEqual(1);

    // Facilitator must still be alive
    const health = await fetch(new URL("./health", facilitator.url));
    expect(health.status).toBe(200);
  }, 120_000);
});

// ============================================================================
// Resilience burst
// ============================================================================

describe("Facilitator resilience", () => {
  it("50 rapid /verify requests (round-robin wallets) → facilitator still healthy", async () => {
    const pool = new WalletPool();
    const requests = await Promise.all(
      Array.from({ length: 50 }, async () => {
        const { account } = pool.next();
        return buildPaymentRequest(account, opts());
      })
    );

    // Fire all 50 concurrently
    const results = await Promise.allSettled(
      requests.map((req) => verifyPayment(facilitator.url, req))
    );

    const fulfilled = results.filter((r) => r.status === "fulfilled").length;
    console.log(`Burst: ${fulfilled}/50 verify requests fulfilled`);

    // Facilitator must still respond to /health
    const health = await fetch(new URL("./health", facilitator.url));
    expect(health.status).toBe(200);
  }, 60_000);
});
