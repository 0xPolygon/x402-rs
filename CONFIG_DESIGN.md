# Config Design: Security & 12-Factor Compliance

This document explains how the facilitator satisfies all deployment constraints:
- Private keys never written to disk
- No config file mounting required
- No deployment changes from previous setup
- 12-factor app compliant

---

## How it works

The binary accepts a JSON config file path via `--config`. The config file contains
`$VAR` references instead of actual secret values. At startup, the binary resolves
those references from the process environment directly into memory.

The config file is baked into the Docker image at build time. It contains no secrets
and is safe to commit and ship in the image.

---

## 1. Keys never touch disk

**`config.amoy.json` lines 8–31** — the file stored in the image contains only
variable name strings, not key values:

```json
"signers": [
  "$POLYGON_SIGNER_1",
  "$POLYGON_SIGNER_2",
  ...
  "$POLYGON_SIGNER_24"
]
```

**`crates/x402-types/src/config.rs` lines 146–165** — at runtime, `LiteralOrEnv`
deserialisation detects the `$VAR` syntax and calls `std::env::var()` directly,
storing the result in heap memory. No write to disk occurs anywhere in this path:

```rust
let s = String::deserialize(deserializer)?;          // reads "$POLYGON_SIGNER_1"
if let Some(var_name) = Self::parse_env_var_syntax(&s) {
    std::env::var(&var_name)                         // reads from process env
        .map_err(...)?                               // errors if var missing
} else { s }
// result parsed into type T and stored in memory — never written to disk
```

---

## 2. No config file mounting required

**`Dockerfile` line 26** — the config file is copied into the image at build time:

```dockerfile
COPY config.amoy.json /app/config.amoy.json
```

**`Dockerfile` line 31** — the binary is pointed at it directly via CLI arg:

```dockerfile
ENTRYPOINT ["x402-facilitator", "--config", "/app/config.amoy.json"]
```

No Kargo volume mount, no file injection pipeline, no `CONFIG` env var needed.

---

## 3. No deployment changes

**`config.amoy.json` lines 8–31** — the same `$POLYGON_SIGNER_1` through
`$POLYGON_SIGNER_24` env var names that Kargo already injects. The contract
between Kargo and the container is unchanged.

**`crates/x402-types/src/config.rs` line 150** — resolution uses the standard
`std::env::var()` call, so any env var injected by Kargo is picked up
automatically at startup.

---

## 4. 12-factor compliance

**`crates/x402-types/src/config.rs` lines 146–165** — the full resolution flow:

```
JSON file  →  detect "$VAR" syntax  →  std::env::var()  →  parse into type T  →  heap memory
```

The config file is structural (chain ID, schemes, RPC URL) — none of it is secret.
All secrets live exclusively in the process environment, injected by Kargo.
The binary never writes them anywhere.

This satisfies the [12-factor config rule](https://12factor.net/config):
> "Store config in the environment. Strict separation of config from code."

---

## File reference summary

| File | Lines | What it shows |
|------|-------|---------------|
| `config.amoy.json` | 8–31 | `$POLYGON_SIGNER_N` references — no real keys in image |
| `config.amoy.json` | 34 | `$AMOY_RPC_URL` reference — resolved from env at runtime |
| `Dockerfile` | 26 | Config file baked into image at build time |
| `Dockerfile` | 31 | Binary pointed at config via `--config` CLI arg |
| `crates/chains/x402-chain-eip155/src/chain/config.rs` | 102–108 | `RpcConfig.http` typed as `LiteralOrEnv<Url>` — supports `$VAR` references |
| `crates/x402-types/src/config.rs` | 75–120 | `LiteralOrEnv` — `$VAR` / `${VAR}` syntax parsing |
| `crates/x402-types/src/config.rs` | 146–165 | `std::env::var()` resolution into process memory |
