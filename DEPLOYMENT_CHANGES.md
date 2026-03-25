# Deployment Changes: Pre-Sync → Upstream v1.1.1

This document lists every change that affects how the facilitator is deployed and
operated. 

---

## TL;DR — What `config.amoy.json` Solves

**Most of the deployment changes below are already resolved by [`config.amoy.json`](config.amoy.json) in this repo.**

| Change | What it fixes | Handled in `config.amoy.json`? |
|--------|--------------|-------------------------------|
| 1 — Signer injection | 24 signers as `$POLYGON_SIGNER_N` env-var references | ✅ lines 8–31 |
| 4 — Schemes must be explicit | `"schemes"` array with `v1-eip155-exact` + `v2-eip155-exact` | ✅ lines 38–41 |
| 6 — `eip1559: false` for Amoy | Polygon Amoy does not support EIP-1559, must opt out | ✅ line 6 |
| 7 — Amoy network + USDC address | Chain ID `eip155:80002` as the config map key | ✅ line 5 |

**What `config.amoy.json` does NOT solve (needs Kargo action):**

| Change | Required Kargo action |
|--------|-----------------------|
| 2 — Config file mount | Mount `config.amoy.json` into the container and set `CONFIG=/path/to/config.amoy.json` (or rename to `config.json` at `WORKDIR`) |
| 3 — New container image | Update Kargo image reference from `docker.io/ukstv/x402-facilitator` → `ghcr.io/0xpolygon/x402-facilitator` |
| 9 — Telemetry (optional) | Add `OTEL_*` env vars if you want distributed tracing; safe to skip |

Changes 5 and 8 require no deployment action (internal code change / client-side concern).

---

## 1. Private Key / Signer Injection — BREAKING CHANGE

### Before (pre-sync)
Keys were likely passed as individual top-level environment variables that the
binary read directly (e.g. `POLYGON_SIGNER_1`, `POLYGON_SIGNER_2`).

### After (v1.1.1)
Keys are **no longer read from environment variables directly**. The facilitator
reads from a **JSON config file**. Environment variables are supported, but only
when explicitly **referenced inside the config file** using `$VAR` or `${VAR}`
syntax.

**Required change:** Your Kargo pipeline must generate (or mount) a `config.json`
that lists each signer as an env-var reference:

```json
{
  "chains": {
    "eip155:80002": {
      "eip1559": false,
      "signers": [
        "$POLYGON_SIGNER_1",
        "$POLYGON_SIGNER_2",
        "$POLYGON_SIGNER_3"
      ],
      "rpc": [
        { "http": "${AMOY_RPC_URL}", "rate_limit": 20 }
      ]
    }
  },
  "schemes": [
    { "id": "v1-eip155-exact", "chains": "eip155:*" },
    { "id": "v2-eip155-exact", "chains": "eip155:*" }
  ]
}
```

The binary resolves `$POLYGON_SIGNER_1` at startup from the process environment.
Your existing environment variable names (`POLYGON_SIGNER_1..24`) are unchanged —
only the config file needs to reference them.

**Why this works — code trace:**

1. **[`facilitator/src/run.rs:79`](facilitator/src/run.rs#L79) — entry point**
   ```rust
   let config = Config::load()?;
   ```
   `run()` is called from `main()`. The very first thing it does after loading
   `.env` is call `Config::load()`.

2. **[`crates/x402-types/src/config.rs:291-304`](crates/x402-types/src/config.rs#L291-L304) — file read**
   ```rust
   pub fn load() -> Result<Self, ConfigError> {
       let cli_args = CliArgs::parse();  // reads --config or CONFIG env var for the path
       Self::load_from_path(...)
   }
   pub fn load_from_path(path: PathBuf) -> Result<Self, ConfigError> {
       let content = fs::read_to_string(&path)?;       // reads the JSON file from disk
       let config = serde_json::from_str(&content)?;   // triggers serde deserialization
       Ok(config)
   }
   ```

3. **[`crates/chains/x402-chain-eip155/src/chain/config.rs:129`](crates/chains/x402-chain-eip155/src/chain/config.rs#L129) — signers type**
   ```rust
   pub type Eip155SignersConfig = Vec<LiteralOrEnv<EvmPrivateKey>>;
   ```
   The `signers` field is typed as `Vec<LiteralOrEnv<EvmPrivateKey>>`. Every
   array entry is deserialized through the `LiteralOrEnv` wrapper — there is no
   other code path for loading signers.

4. **[`crates/x402-types/src/config.rs:105-120`](crates/x402-types/src/config.rs#L105-L120) — `$VAR` detection**
   ```rust
   fn parse_env_var_syntax(s: &str) -> Option<String> {
       if s.starts_with("${") && s.ends_with('}') {
           Some(s[2..s.len() - 1].to_string())   // ${VAR_NAME}
       } else if s.starts_with('$') && s.len() > 1 {
           Some(s[1..].to_string())               // $VAR_NAME
       } else {
           None                                    // literal hex key — no lookup
       }
   }
   ```

5. **[`crates/x402-types/src/config.rs:149-155`](crates/x402-types/src/config.rs#L149-L155) — env var resolved**
   ```rust
   let value = if let Some(var_name) = Self::parse_env_var_syntax(&s) {
       std::env::var(&var_name)   // ← looks up POLYGON_SIGNER_1 from process env
           .map_err(|_| serde::de::Error::custom(
               format!("Environment variable '{}' not found", var_name)
           ))?
   } else {
       s  // ← used as a literal hex key if no $ prefix
   };
   ```

**Full startup flow:**
```
config.json  ──(fs::read_to_string)──▶  JSON string
                                              │
                                    serde_json::from_str()
                                              │
                                  LiteralOrEnv::deserialize()
                                              │
                             parse_env_var_syntax("$POLYGON_SIGNER_1")
                                              │
                             std::env::var("POLYGON_SIGNER_1")  ──▶  raw hex
                                              │
                             EvmPrivateKey::from_str(hex)  ──▶  validated key
```
The private key value never appears in the config file. If the env var is missing
at startup, the binary exits immediately with a deserialization error naming the
missing variable.

---

## 2. Config File Loading

The config file path is resolved in this order of priority:

| Priority | Source |
|----------|--------|
| 1 | `--config <path>` CLI argument |
| 2 | `-c <path>` CLI argument (short form) |
| 3 | `CONFIG` environment variable |
| 4 | `config.json` in the working directory (default) |

**Kargo recommendation:** Mount the rendered config as a `ConfigMap`/`Secret`
volume and pass its path via `--config /etc/x402/config.json` or the `CONFIG`
env var.

**Why — code trace:**

**[`crates/x402-types/src/config.rs:183-194`](crates/x402-types/src/config.rs#L183-L194) — CLI arg definition**
```rust
pub struct CliArgs {
    #[cfg_attr(
        feature = "cli",
        arg(long, short, env = "CONFIG", default_value = "config.json")
    )]
    pub config: PathBuf,
}
```
`clap` resolves the value in this precedence: explicit `--config` flag →
`-c` short flag → `CONFIG` env var → hardcoded default `"config.json"`.
The `env = "CONFIG"` attribute is what makes the env var work — clap reads it
automatically, not custom code.

---

## 3. Container Image Registry — ACTION IN INFRA REPO

### Before (v0.12.x)
Images were pushed to two registries:
- `ghcr.io/x402-rs/x402-facilitator` (upstream GHCR org)
- `docker.io/ukstv/x402-facilitator` (Docker Hub mirror)

### After (v1.1.1)
This repo's CI already pushes to the correct place — **no change needed in this
repo**. Action is only required in your infra/GitOps repo if Kargo still points
at the old upstream image names:

| | Old (upstream) | New (this repo) |
|--|--------|-------|
| GHCR | `ghcr.io/x402-rs/x402-facilitator` | `ghcr.io/0xpolygon/x402-facilitator` |
| Docker Hub | `docker.io/ukstv/x402-facilitator` | removed — GHCR only |
| Auth | upstream org token | `0xpolygon` org GITHUB_TOKEN |
| Architectures | `amd64` + `arm64` | `amd64` + `arm64` (unchanged) |

**Why — code trace:**

**Before [`v0.12.6:.github/workflows/ci.yaml`]**
```yaml
env:
  IMAGE_NAME: x402-rs/x402-facilitator       # ← upstream org
  IMAGE_NAME_DH: ukstv/x402-facilitator      # ← Docker Hub (now gone)
```
The `merge-manifest` job pushed to both `ghcr.io/$IMAGE_NAME` and
`docker.io/$IMAGE_NAME_DH` simultaneously.

**After [`.github/workflows/ci.yaml:8-9`](.github/workflows/ci.yaml#L8-L9)**
```yaml
env:
  REGISTRY: ghcr.io
  IMAGE_NAME: 0xpolygon/x402-facilitator     # ← Polygon org, GHCR only
```
The `IMAGE_NAME_DH` variable and the entire Docker Hub push step have been
removed. Only `ghcr.io/0xpolygon/x402-facilitator` is published.

---

## 4. Image Tagging Strategy — CHANGED

### Before (v0.12.x)
```yaml
# On push to main:
tags: type=raw,value=dev          # → :dev tag

# On version tag v*:
tags: |
  type=raw,value=latest           # → :latest tag (EXISTED before)
  type=semver,pattern={{version}} # → 1.1.1
  ...
```
Two registries were published simultaneously:
- `ghcr.io/x402-rs/x402-facilitator:latest`
- `docker.io/ukstv/x402-facilitator:latest` (Docker Hub mirror)

### After (v1.1.1)
```yaml
# On push to main:
tags: type=raw,value=${{ github.sha }}   # → full 40-char SHA

# On version tag v*:
tags: |
  type=semver,pattern={{version}}         # → 1.1.1
  type=semver,pattern={{major}}.{{minor}} # → 1.1
  type=semver,pattern={{major}}           # → 1
  # no :latest, no :dev
```
Only one registry: `ghcr.io/0xpolygon/x402-facilitator`. Docker Hub is gone.

### Deployment impact
- **If your Kargo `ImageRepository` polls for `:latest`** — it will never see a
  new image. You must change to polling for semver tags or the SHA pattern.
- **If your Kargo `Stage` pins `:dev`** — that tag no longer exists.
- **Recommended Kargo policy:** use semver constraint `>= 1.1.1` or pin to an
  exact tag like `1.1.1`.

**Why — code trace:**

**Before [`v0.12.6:.github/workflows/ci.yaml`](https://github.com/0xPolygon/x402-facilitator/blob/v0.12.6/.github/workflows/ci.yaml)**
```yaml
env:
  IMAGE_NAME: x402-rs/x402-facilitator       # old GHCR org
  IMAGE_NAME_DH: ukstv/x402-facilitator      # Docker Hub — pushed in merge-manifest job

tags: |
  type=raw,value=latest    # ← :latest existed on release tags
```

**After [`.github/workflows/ci.yaml:8-9,32-47`](.github/workflows/ci.yaml#L8-L9)**
```yaml
env:
  IMAGE_NAME: 0xpolygon/x402-facilitator     # new org, GHCR only

# main branch — SHA only, no :dev
tags: type=raw,value=${{ github.sha }}

# release tag — semver only, no :latest
tags: |
  type=semver,pattern={{version}}
  type=semver,pattern={{major}}.{{minor}}
  type=semver,pattern={{major}}
```
`type=raw,value=latest` and the entire Docker Hub push block were removed. No
`flavor: latest=true` is used either. The `:latest` tag simply does not exist
in the new pipeline.

---

## 5. Port / Host Configuration — NO CHANGE NEEDED

### Before (v0.12.x)
`PORT`, `HOST` were read via `env::var("PORT")` as serde defaults — identical
to the current version. The behaviour has not changed.

### After (v1.1.1)
Identical. Port and host can still be set three ways (in precedence order):

| Priority | Method | Example |
|----------|--------|---------|
| 1 | Config file field | `"port": 8080` |
| 2 | Environment variable | `PORT=8080` |
| 3 | Hardcoded default | `8080` / `0.0.0.0` |

No action required. If you are already injecting `PORT` as an env var it
continues to work exactly as before.

**Why — code trace (unchanged between versions):**

**[`crates/x402-types/src/config.rs:226-247`](crates/x402-types/src/config.rs#L226-L247)**
```rust
pub fn default_port() -> u16 {
    env::var("PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_PORT)
}
pub fn default_host() -> IpAddr {
    env::var("HOST").ok().and_then(|s| s.parse().ok())
        .unwrap_or(IpAddr::V4(DEFAULT_HOST.parse().unwrap()))
}
```
This same pattern existed in `v0.12.6:src/config.rs` unchanged. The only
difference is it now lives in the `x402-types` crate instead of `src/config.rs`
(a code-reorganisation change, not a behaviour change).

**[`Dockerfile:3,18`](Dockerfile#L3)**
```dockerfile
ENV PORT=8080    # default baked into image — unchanged
ENV RUST_LOG=info
```

---

## 6. Chain Configuration Structure — FIELD NAMES UNCHANGED, `eip1559` default matters

### Before (v0.12.x)
The chain config struct fields (`eip1559`, `flashblocks`, `signers`, `rpc`,
`receipt_timeout_secs`) existed with the same names and the same defaults.
This section of the config has **not changed**.

### After (v1.1.1)
Same field names, same defaults. The struct has been moved from `src/config.rs`
into the `x402-chain-eip155` crate, but the JSON shape is identical.

### Deployment impact
The fields themselves did not change. **The actionable point is:**
- `eip1559` defaults to `true`
- Polygon Amoy **does not** support EIP-1559 pricing
- You **must** explicitly set `"eip1559": false` in your Amoy chain config or
  gas estimation will fail on every transaction

**Why — code trace:**

**[`crates/chains/x402-chain-eip155/src/chain/config.rs:70-98`](crates/chains/x402-chain-eip155/src/chain/config.rs#L70-L98)**
```rust
pub struct Eip155ChainConfigInner {
    #[serde(default = "eip155_chain_config::default_eip1559")]    // ← defaults true
    pub eip1559: bool,
    #[serde(default = "eip155_chain_config::default_flashblocks")] // ← defaults false
    pub flashblocks: bool,
    #[serde(default = "eip155_chain_config::default_receipt_timeout_secs")] // ← 30s
    pub receipt_timeout_secs: u64,
    pub signers: Eip155SignersConfig,  // required — no default, startup error if missing
    pub rpc: Vec<RpcConfig>,           // required — no default, startup error if missing
}
```
This is **the same struct definition** that existed in `v0.12.6:src/config.rs`.
It was copied verbatim into the new crate layout. Nothing changed.

The CAIP-2 key routing also existed in v0.12.x in `src/config.rs` — it has
simply moved to [`facilitator/src/config.rs`](facilitator/src/config.rs#L128-L205):
```rust
while let Some(chain_id) = access.next_key::<ChainId>()? {
    match chain_id.namespace() {
        "eip155"  => { /* Eip155ChainConfigInner */ }
        "solana"  => { /* SolanaChainConfigInner */ }
        "aptos"   => { /* AptosChainConfigInner  */ }
        other => return Err(custom(format!("Unexpected namespace: {}", other))),
    }
}
```

---

## 7. Schemes Configuration — BREAKING CHANGE (implicit → explicit)

### Before (v0.12.x)
`SchemeBlueprints::full()` was called unconditionally at startup, which
**automatically registered every scheme the binary was compiled with**:

```rust
// v0.12.6:src/main.rs:73
let scheme_blueprints = SchemeBlueprints::full();  // ← all schemes enabled automatically
let scheme_registry = SchemeRegistry::build(chain_registry, scheme_blueprints, config.schemes());
```

Even if `config.schemes` was empty (`[]`) or absent, the registry would still
activate every scheme that had a matching chain provider. Schemes were
opt-out, not opt-in.

### After (v1.1.1)
`SchemeBlueprints::full()` has been removed. Each scheme is now **individually
registered** as a blueprint, and **only activates if a matching entry exists in
`config.schemes`**:

```rust
// facilitator/src/run.rs:82-99
let mut scheme_blueprints = SchemeBlueprints::new();   // ← empty by default
scheme_blueprints.register(V1Eip155Exact);             // blueprint registered...
scheme_blueprints.register(V2Eip155Exact);             // ...but NOT yet active
// ...
let scheme_registry = SchemeRegistry::build(
    chain_registry, scheme_blueprints, config.schemes()  // ← config.schemes() drives activation
);
```

### Deployment impact
**If your config has no `schemes` array (or an empty one), no payment schemes
will be active** — `/verify` and `/settle` will return 400 for everything.

You must declare both schemes explicitly:
```json
"schemes": [
  { "id": "v1-eip155-exact", "chains": "eip155:*" },
  { "id": "v2-eip155-exact", "chains": "eip155:*" }
]
```

Supported scheme IDs: `v1-eip155-exact`, `v2-eip155-exact`, `v1-solana-exact`,
`v2-solana-exact`, `v2-aptos-exact`.

Chain patterns: `eip155:*` (all EVM), `eip155:80002` (specific chain),
`eip155:{80002,137}` (set).

**Why — code trace:**

**Before [`v0.12.6:src/main.rs:73`]**
```rust
let scheme_blueprints = SchemeBlueprints::full();
```
`full()` registered everything. Even with an empty `schemes: []` in config, a
scheme would still activate if its chain was configured.

**After [`facilitator/src/run.rs:82-102`](facilitator/src/run.rs#L82-L102)**
```rust
let mut scheme_blueprints = SchemeBlueprints::new();  // starts empty
scheme_blueprints.register(V1Eip155Exact);
scheme_blueprints.register(V2Eip155Exact);
// ...
let scheme_registry = SchemeRegistry::build(chain_registry, scheme_blueprints, config.schemes());
```

**[`crates/x402-types/src/scheme/mod.rs:248-302`](crates/x402-types/src/scheme/mod.rs#L248-L302) — `SchemeRegistry::build()`**
```rust
for config in config {                                   // iterates config.schemes entries
    if !config.enabled { continue; }
    let blueprint = blueprints.get(&config.id)?;        // looks up blueprint by id
    let chain_providers = chains.by_chain_id_pattern(&config.chains);
    if chain_providers.is_empty() { continue; }
    // only if both found → build and register handler
}
```
No entry in `config.schemes` = no handler built = scheme silently inactive.

---

## 8. Bug Fix: Payment Value Serde (v1.1.0) — CLIENT REVIEW NEEDED

### Before (v0.12.x / v1.0.x)
The `value` field in EIP-155 payment payloads was typed as `U256`, which alloy
serializes as a **hex string** (e.g. `"0x3e8"`). This caused a mismatch when
clients sent decimal strings.

### After (v1.1.0 / v1.1.1)
`value` is now typed as `TokenAmount`, which serializes/deserializes as a
**decimal string** (e.g. `"1000"`).

### Deployment impact
No config change required for the facilitator itself. However:
- Any downstream client that was **sending `value` as a hex string** to work
  around the old bug must be updated to send a decimal string instead
- Our test suite in `amoy-payment.ts` already sends decimal strings:
  `value: opts.amount.toString()` — this is correct

**Why — code trace:**

**Commit `f38df17`** — `chore(x402-chain-eip155): replace U256 with TokenAmount for value field`

The `TokenAmount` type serializes as a decimal string:
[`crates/x402-types/src/token_amount.rs`](crates/x402-types/src/token_amount.rs)

Before (broken):
```rust
pub value: U256,   // alloy serializes U256 as "0x3e8" (hex)
```
After (fixed):
```rust
pub value: TokenAmount,  // serializes as "1000" (decimal string)
```

---

## 9. Observability — TELEMETRY NOW CONDITIONAL

### Before (v0.12.x)
Telemetry was **always initialised unconditionally** at startup, regardless of
whether `OTEL_EXPORTER_OTLP_ENDPOINT` was set:

```rust
// v0.12.6:src/main.rs
let telemetry = Telemetry::new()
    .with_name(env!("CARGO_PKG_NAME"))
    .with_version(env!("CARGO_PKG_VERSION"))
    .register();   // ← always called, even if no OTEL endpoint configured
```

The binary also had no feature flag — telemetry code was always compiled in and
always ran.

### After (v1.1.1)
Telemetry is now **behind a compile-time feature flag** (`telemetry`) and is
only initialised when the feature is enabled. The Dockerfile still compiles with
`--features full` so the code is present, but the OTEL exporter only activates
when `OTEL_EXPORTER_OTLP_ENDPOINT` is set in the environment:

```rust
// facilitator/src/run.rs
#[cfg(feature = "telemetry")]   // ← compile-time gate
let telemetry_layer = { Telemetry::new()...register()...http_tracing() };
```

If `OTEL_EXPORTER_OTLP_ENDPOINT` is absent, the binary logs
`"OpenTelemetry is not enabled"` and continues without tracing.

### Deployment impact
- No change needed if you are already setting `OTEL_EXPORTER_OTLP_ENDPOINT`
- If you were not setting it before and telemetry was failing silently or
  printing errors, those are now suppressed cleanly

OpenTelemetry tracing is **compiled into the binary** (via the `telemetry` feature
in `Dockerfile`) but only activates when `OTEL_EXPORTER_OTLP_ENDPOINT` is set.

```
OTEL_EXPORTER_OTLP_ENDPOINT=http://your-collector:4317
OTEL_EXPORTER_OTLP_PROTOCOL=grpc        # or http/protobuf
OTEL_SERVICE_NAME=x402-facilitator
OTEL_SERVICE_VERSION=1.1.1
OTEL_SERVICE_DEPLOYMENT=production
RUST_LOG=info                            # log level — set in Dockerfile
```

**Why — code trace:**

**[`Dockerfile:13`](Dockerfile#L13) — telemetry compiled in**
```dockerfile
RUN cargo build --package x402-facilitator --features full --release --locked
```
`--features full` includes the `telemetry` feature, so the OTEL code is present
in the shipped binary.

**[`facilitator/src/run.rs:67-77`](facilitator/src/run.rs#L67-L77) — conditional activation**
```rust
dotenv().ok();   // loads .env file if present

#[cfg(feature = "telemetry")]
let telemetry_layer = {
    let telemetry = Telemetry::new()
        .with_name(env!("CARGO_PKG_NAME"))
        .with_version(env!("CARGO_PKG_VERSION"))
        .register();     // reads OTEL_* env vars here
    telemetry.http_tracing()
};
```
If `OTEL_EXPORTER_OTLP_ENDPOINT` is not set, the OTLP exporter is a no-op and
no traces are exported. The binary logs a message: `"OpenTelemetry is not enabled"`.

**[`crates/x402-facilitator-local/src/util/telemetry.rs:13-17`](crates/x402-facilitator-local/src/util/telemetry.rs#L13-L17) — full env var list**
```
OTEL_EXPORTER_OTLP_ENDPOINT   OTLP collector endpoint
OTEL_EXPORTER_OTLP_PROTOCOL   Protocol (http/protobuf or grpc)
OTEL_SERVICE_NAME              Service name for traces
OTEL_SERVICE_VERSION           Service version
OTEL_SERVICE_DEPLOYMENT        Deployment environment
```

**Graceful shutdown** is also wired in at this layer:
**[`facilitator/src/run.rs:126-131`](facilitator/src/run.rs#L126-L131)**
```rust
let sig_down = SigDown::try_new()?;
let axum_cancellation_token = sig_down.cancellation_token();
axum::serve(listener, http_endpoints)
    .with_graceful_shutdown(async move {
        axum_cancellation_token.cancelled().await
    })
    .await?;
```
`SigDown` listens for `SIGTERM` and `SIGINT`
([`crates/x402-facilitator-local/src/util/sig_down.rs`](crates/x402-facilitator-local/src/util/sig_down.rs)).
Kargo/Kubernetes sends `SIGTERM` on pod termination — the server will drain
in-flight requests before exiting cleanly.

---

## 10. Full Polygon Amoy Config Example

See [`config.amoy.json`](config.amoy.json) for the ready-to-use file.

```json
{
  "host": "0.0.0.0",
  "port": 8080,
  "chains": {
    "eip155:80002": {
      "eip1559": false,
      "signers": [
        "$POLYGON_SIGNER_1",  "$POLYGON_SIGNER_2",  "$POLYGON_SIGNER_3",
        "$POLYGON_SIGNER_4",  "$POLYGON_SIGNER_5",  "$POLYGON_SIGNER_6",
        "$POLYGON_SIGNER_7",  "$POLYGON_SIGNER_8",  "$POLYGON_SIGNER_9",
        "$POLYGON_SIGNER_10", "$POLYGON_SIGNER_11", "$POLYGON_SIGNER_12",
        "$POLYGON_SIGNER_13", "$POLYGON_SIGNER_14", "$POLYGON_SIGNER_15",
        "$POLYGON_SIGNER_16", "$POLYGON_SIGNER_17", "$POLYGON_SIGNER_18",
        "$POLYGON_SIGNER_19", "$POLYGON_SIGNER_20", "$POLYGON_SIGNER_21",
        "$POLYGON_SIGNER_22", "$POLYGON_SIGNER_23", "$POLYGON_SIGNER_24"
      ],
      "rpc": [
        { "http": "${AMOY_RPC_URL}", "rate_limit": 20 }
      ]
    }
  },
  "schemes": [
    { "id": "v1-eip155-exact", "chains": "eip155:*" },
    { "id": "v2-eip155-exact", "chains": "eip155:*" }
  ]
}
```

The keys `POLYGON_SIGNER_1..24` and `AMOY_RPC_URL` must be present in the
container's environment when the facilitator starts.

---

## Summary Checklist

- [ ] In your infra/GitOps repo: update Kargo `ImageRepository` if it still points at `ukstv/` or `x402-rs/` upstream images (this repo's CI already pushes to `ghcr.io/0xpolygon/x402-facilitator`)
- [ ] Create/mount a `config.json` file using `$VAR` references for all signer keys
- [ ] Pass config path via `--config` arg or `CONFIG` env var
- [ ] Set `eip1559: false` for Polygon Amoy chain config
- [ ] Declare both `v1-eip155-exact` and `v2-eip155-exact` in `schemes`
- [ ] No `latest` tag — pin Kargo to semver tag (e.g. `1.1.1`)
- [ ] Ensure `POLYGON_SIGNER_1..24` are still injected as env vars (names unchanged)
- [ ] Optionally set `OTEL_EXPORTER_OTLP_ENDPOINT` and `RUST_LOG` for observability
