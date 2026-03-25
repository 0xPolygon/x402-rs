/**
 * Amoy Load Tests
 *
 * Sustained concurrent and sequential load against the facilitator.
 * One facilitator process is spawned for the entire file.
 *
 * Assertions:
 * - 0% failure rate on verify
 * - p95 settle latency < 60s (Amoy block time ~2s)
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

function opts(scheme: PaymentOptions["scheme"] = "v1-eip155-exact"): PaymentOptions {
  return { amount: PAYMENT_AMOUNT, payTo: PAY_TO, scheme };
}

function percentile(sorted: number[], p: number): number {
  const idx = Math.ceil((p / 100) * sorted.length) - 1;
  return sorted[Math.max(0, idx)];
}

beforeAll(async () => {
  const configPath = await makeAmoyFacilitatorConfig();
  facilitator = await RSFacilitatorHandle.spawn(undefined, configPath);
}, 120_000);

afterAll(async () => {
  await facilitator.stop();
});

// ============================================================================
// Concurrent /verify
// ============================================================================

describe("Concurrent verify", () => {
  it("10 concurrent /verify requests (one per buyer wallet) — all succeed within 30s", async () => {
    const pool = new WalletPool();
    const requests = await Promise.all(
      Array.from({ length: 10 }, async () => {
        const { account } = pool.next();
        return buildPaymentRequest(account, opts());
      })
    );

    const start = Date.now();
    const results = await Promise.all(
      requests.map((req) => verifyPayment(facilitator.url, req))
    );
    const elapsed = Date.now() - start;

    console.log(`10 concurrent verifies completed in ${elapsed}ms`);
    expect(elapsed).toBeLessThan(30_000);
    for (const { status, body } of results) {
      expect(status).toBe(200);
      expect(body.isValid).toBe(true);
    }
  }, 60_000);
});

// ============================================================================
// Sequential settle burst
// ============================================================================

describe("Sequential settle burst", () => {
  it("10 sequential settlements — p95 latency < 60s", async () => {
    const pool = new WalletPool();
    const latencies: number[] = [];

    for (let i = 0; i < 10; i++) {
      const { account } = pool.next();
      const req = await buildPaymentRequest(account, opts());
      const t0 = Date.now();
      const { status, body } = await settlePayment(facilitator.url, req);
      latencies.push(Date.now() - t0);
      console.log(`[seq settle ${i}] tx: ${body.transaction} https://amoy.polygonscan.com/tx/${body.transaction}`);
      expect(status, `settlement ${i} failed`).toBe(200);
      expect(body.success, `settlement ${i} not successful`).toBe(true);
    }

    const sorted = [...latencies].sort((a, b) => a - b);
    const p50 = percentile(sorted, 50);
    const p95 = percentile(sorted, 95);
    console.log(`Settle latencies — p50: ${p50}ms, p95: ${p95}ms, max: ${sorted[sorted.length - 1]}ms`);
    expect(p95).toBeLessThan(60_000);
  }, 600_000);
});

// ============================================================================
// Parallel verify+settle pairs
// ============================================================================

describe("Parallel verify+settle pairs", () => {
  it("10 parallel verify+settle pairs (all buyer wallets) — 0% failure rate", async () => {
    const pool = new WalletPool();

    const results = await Promise.all(
      Array.from({ length: 10 }, async (_, i) => {
        const { account } = pool.next();
        const req = await buildPaymentRequest(account, opts());
        const verify = await verifyPayment(facilitator.url, req);
        if (!verify.body.isValid) return { ok: false, index: i, phase: "verify" };
        const settle = await settlePayment(facilitator.url, req);
        console.log(`[parallel pair ${i}] tx: ${settle.body.transaction} https://amoy.polygonscan.com/tx/${settle.body.transaction}`);
        if (!settle.body.success) return { ok: false, index: i, phase: "settle" };
        return { ok: true, index: i, phase: "done" };
      })
    );

    const failures = results.filter((r) => !r.ok);
    console.log(`${results.length - failures.length}/10 pairs succeeded`);
    if (failures.length > 0) {
      console.error("Failures:", failures);
    }
    expect(failures).toHaveLength(0);
  }, 300_000);
});

// ============================================================================
// /supported endpoint flood
// ============================================================================

describe("/supported endpoint flood", () => {
  it("50 concurrent GET /supported → all 200", async () => {
    const results = await Promise.all(
      Array.from({ length: 50 }, () =>
        fetch(new URL("./supported", facilitator.url)).then((r) => r.status)
      )
    );
    const nonOk = results.filter((s) => s !== 200);
    console.log(`50 concurrent /supported: ${results.filter(s => s === 200).length} OK, ${nonOk.length} failed`);
    expect(nonOk).toHaveLength(0);
  }, 30_000);
});
