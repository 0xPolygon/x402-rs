/**
 * Amoy-only configuration — no Base Sepolia or Solana dependencies.
 * Import this instead of config.ts in Amoy test files.
 */

import dotenv from "dotenv";
import tmp from "tmp";
import * as fs from "node:fs/promises";
import * as path from "node:path";

// Load root .env (contains POLYGON_SIGNER_* keys)
dotenv.config({ path: path.resolve(process.cwd(), "../.env") });
dotenv.config(); // also load local .env if present

const AMOY_RPC_URL =
  process.env.AMOY_RPC_URL ?? "https://rpc-amoy.polygon.technology";

function readAmoyKey(n: number): `0x${string}` {
  const key = process.env[`POLYGON_SIGNER_${n}`];
  if (!key) throw new Error(`POLYGON_SIGNER_${n} is not set in .env`);
  return key as `0x${string}`;
}

// POLYGON_SIGNER_1..24 — have POL (gas), no USDC → facilitator signers
export const amoySignerKeys: `0x${string}`[] = Array.from({ length: 24 }, (_, i) =>
  readAmoyKey(i + 1)
);

// POLYGON_SIGNER_25..34 — have 20 USDC each, no POL → buyer pool
export const amoyBuyerKeys: `0x${string}`[] = Array.from({ length: 10 }, (_, i) =>
  readAmoyKey(i + 25)
);

export const AMOY_FACILITATOR_CONFIG = {
  host: "0.0.0.0",
  chains: {
    "eip155:80002": {
      eip1559: false,
      signers: amoySignerKeys,
      rpc: [{ http: AMOY_RPC_URL, rate_limit: 20 }],
    },
  },
  schemes: [
    { id: "v1-eip155-exact", chains: "eip155:*" },
    { id: "v2-eip155-exact", chains: "eip155:*" },
  ],
};

export async function makeAmoyFacilitatorConfig(): Promise<string> {
  const filename = await new Promise<string>((resolve, reject) => {
    tmp.file({ postfix: ".json" }, (err, tmpPath) => {
      if (err) reject(err);
      else resolve(tmpPath);
    });
  });
  await fs.writeFile(filename, JSON.stringify(AMOY_FACILITATOR_CONFIG, null, 2));
  return filename;
}
