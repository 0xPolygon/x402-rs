/**
 * Docker facilitator test — hits the already-running container at localhost:8080.
 * No binary spawning. Buyer wallets: POLYGON_SIGNER_25..34 (have test USDC).
 */

import { describe, it, expect, beforeAll } from "vitest";
import dotenv from "dotenv";
import * as path from "node:path";
import { privateKeyToAccount } from "viem/accounts";
import {
  buildPaymentRequest,
  verifyPayment,
  settlePayment,
  type PaymentOptions,
} from "../utils/amoy-payment.js";

dotenv.config({ path: path.resolve(process.cwd(), "../.env") });

const FACILITATOR_URL = new URL("http://localhost:8080/");
const PAYMENT_AMOUNT = 1000n; // 0.001 USDC
const PAY_TO = "0xc50f9F01159BeA6901F3F8dd97C28c7A1663369D" as const;

function readKey(n: number): `0x${string}` {
  const key = process.env[`POLYGON_SIGNER_${n}`];
  if (!key) throw new Error(`POLYGON_SIGNER_${n} not set in .env`);
  return key as `0x${string}`;
}

// Buyer wallets: SIGNER_25..34 have test USDC
const buyerKeys = Array.from({ length: 10 }, (_, i) => readKey(i + 25));

function nextBuyer(i: number) {
  const key = buyerKeys[i % buyerKeys.length];
  return privateKeyToAccount(key);
}

function opts(scheme: PaymentOptions["scheme"]): PaymentOptions {
  return { amount: PAYMENT_AMOUNT, payTo: PAY_TO, scheme, maxTimeoutSeconds: 300 };
}

beforeAll(async () => {
  const res = await fetch(new URL("./health", FACILITATOR_URL));
  if (!res.ok) throw new Error("Docker facilitator not reachable at localhost:8080");
});

describe("Docker facilitator — health", () => {
  it("GET /health → 200", async () => {
    const res = await fetch(new URL("./health", FACILITATOR_URL));
    expect(res.status).toBe(200);
    const body = await res.json();
    console.log("Health:", JSON.stringify(body, null, 2));
  });
});

describe("Docker facilitator — v1 settle", () => {
  it("settle 0.001 USDC via v1-eip155-exact → tx on Amoy", async () => {
    const account = nextBuyer(0);
    const req = await buildPaymentRequest(account, opts("v1-eip155-exact"));
    const { status, body } = await settlePayment(FACILITATOR_URL, req);
    console.log(`[v1] status=${status} tx=${body.transaction} https://amoy.polygonscan.com/tx/${body.transaction}`);
    expect(status).toBe(200);
    expect(body.success).toBe(true);
    expect(body.transaction).toMatch(/^0x/);
  }, 60_000);
});

describe("Docker facilitator — v2 settle", () => {
  it("settle 0.001 USDC via v2-eip155-exact → tx on Amoy", async () => {
    const account = nextBuyer(1);
    const req = await buildPaymentRequest(account, opts("v2-eip155-exact"));
    const { status, body } = await settlePayment(FACILITATOR_URL, req);
    console.log(`[v2] status=${status} tx=${body.transaction} https://amoy.polygonscan.com/tx/${body.transaction}`);
    expect(status).toBe(200);
    expect(body.success).toBe(true);
    expect(body.transaction).toMatch(/^0x/);
  }, 60_000);
});

describe("Docker facilitator — 5 payments round-robin", () => {
  it("5 sequential 0.001 USDC settlements all succeed", async () => {
    for (let i = 0; i < 5; i++) {
      const account = nextBuyer(i);
      const req = await buildPaymentRequest(account, opts("v1-eip155-exact"));
      const { status, body } = await settlePayment(FACILITATOR_URL, req);
      console.log(`[wallet ${i + 25}] tx=${body.transaction} https://amoy.polygonscan.com/tx/${body.transaction}`);
      expect(status, `wallet ${i} failed: ${JSON.stringify(body)}`).toBe(200);
      expect(body.success).toBe(true);
    }
  }, 300_000);
});
