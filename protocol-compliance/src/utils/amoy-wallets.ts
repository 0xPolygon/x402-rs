import { privateKeyToAccount } from "viem/accounts";
import type { LocalAccount } from "viem";
import { amoyBuyerKeys } from "./amoy-config.js";

export class WalletPool {
  private index = 0;

  next(): { privateKey: `0x${string}`; account: LocalAccount } {
    const key = amoyBuyerKeys[this.index % amoyBuyerKeys.length];
    this.index++;
    return { privateKey: key, account: privateKeyToAccount(key) };
  }

  reset() {
    this.index = 0;
  }
}
