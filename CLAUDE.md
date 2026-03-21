# Project context — Tanda (Soroban Smart Contract)

## Tech stack
- Language: Rust (Soroban SDK v25, `soroban-sdk = "25"`)
- Build target: `wasm32v1-none`
- Build tool: `stellar contract build`
- Test framework: `cargo test` (Soroban testutils; **not** Vitest or Jest)
- Package manager: Cargo (no npm/pnpm/bun — this is a pure contract project)

## Stellar configuration
- Network: **testnet**
- Network passphrase: `Test SDF Network ; September 2015` (`Networks.TESTNET`)
- RPC URL: `https://soroban-testnet.stellar.org`
- USDC issuer (testnet): `GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5`
  (DeFindex-compatible issuer — do NOT use the Circle testnet issuer)
- Local deployer key alias: `alice`
  Public key: `GDTZQTGPOBXU4AA2U3HEQ7RPRBGXKUEZQSAXEGTLSM2CC45BPJLJOB6W`

## Contract: Tanda (ROSCA / Group Savings)

### What it does
Automates a rotating savings group ("tanda"):
- N participants each pay a fixed amount every period.
- Each period the entire pot goes to one member (in registration order).
- 10% of each payment is retained as personal collateral.
- 90% is automatically invested in Etherfuse CETES stablebonds to earn yield.
- On payout, beneficiary receives principal + CETES yield.
- At tanda completion, collateral is returned to each participant.

### Contract source layout
```
contracts/tanda/src/
├── lib.rs          All callable functions (initialize, register, make_payment, …)
├── types.rs        TandaConfig, ParticipantInfo, RoundInfo, InvestmentPool
├── errors.rs       TandaError enum (19 typed errors, #[contracterror])
├── events.rs       On-chain event emitters (one per state change)
└── etherfuse.rs    Cross-contract client for Etherfuse stablebond
```

### Key parameters (set at initialize())
| Parameter | Type | Example | Notes |
|---|---|---|---|
| `max_participants` | u32 | 5 | 2–20 |
| `payment_amount` | i128 | 1_000_000_000 | USDC micro-units (6 dp) = 1000 USDC |
| `period_secs` | u64 | 2_592_000 | 30 days |
| `collateral_bps` | u32 | 1_000 | Hardcoded 10% (1000 bps) |

### Callable functions
```
initialize  register  make_payment  handle_missed_payment
finalize_round  claim_payout  reinvest_payout
get_config  get_round_info  get_investment_pool  get_collateral_pool
get_participant  get_participants  get_turn_order  get_round_cetes
```

### Collateral mechanics
- 10% of each payment → personal `collateral_held` (USDC in contract)
- On missed payment: use participant's own collateral first, then shared pool
- Partial coverage accepted — no hard failure if pool is insufficient
- Collateral returned at end of tanda (Completed status)

## Protocol-specific notes

### Etherfuse (CETES stablebond)
- customer_id: [add your customer_id here] — permanent, never generate a new one
- Auth header: `Authorization: your-api-key` (no `Bearer` prefix)
- Sandbox simulation: POST to `/ramp/order/fiat_received` to advance order state
- Indexing delay: wait 3–10 s after order creation before querying status
- The Etherfuse contract address is stored as `cetes_token` in TandaConfig
- On `make_payment`: 90% USDC is transferred to the Etherfuse contract, then
  `deposit(contract_address, usdc_amount)` is called → returns CETES tokens
- On `claim_payout`: `redeem(contract_address, cetes_amount)` is called →
  Etherfuse transfers USDC + yield back to tanda contract

### DeFindex (if integrated later)
- Auth header: `Authorization: Bearer your-api-key`
- XLM vault address: [add here]
- USDC vault address: [add here]
- Classic Stellar assets must be SAC-deployed before depositing (common ones already are)
- Endpoint is `/vault/` not `/vaults/`; amounts are always arrays; success = HTTP 201

## Deployed contracts (testnet)
| Contract | Address |
|---|---|
| **Tanda** | `CB2U6IECRFVSHXJ2MLRMF6BPFNKYTYA3OAIKCUIA622DEJJQSYBVNNHF` |
| **Mock Etherfuse (CETES)** | `CCERQWDSG5MPZTGPL3NWTYPRNZHFQOQD5H43ZDDUGG2ZATNN7ZEZHCJC` |
| **USDC (SAC)** | `CBIELTK6YBZJU5UP2WWQEUCYKLPU6AUNZ2BQ4WWFEIE3USCIHMXQDAMA` |

Explorer links:
- Tanda: https://lab.stellar.org/r/testnet/contract/CB2U6IECRFVSHXJ2MLRMF6BPFNKYTYA3OAIKCUIA622DEJJQSYBVNNHF
- Mock CETES: https://lab.stellar.org/r/testnet/contract/CCERQWDSG5MPZTGPL3NWTYPRNZHFQOQD5H43ZDDUGG2ZATNN7ZEZHCJC

Current state: **Registering** (3 participants needed, 10 USDC / 30-day period)

## Common commands
```bash
# Build WASM
stellar contract build

# Run all tests (12 tests)
cargo test

# Deploy to testnet
stellar contract deploy \
  --wasm target/wasm32v1-none/release/tanda.wasm \
  --source alice --network testnet

# Initialize (after deploy)
stellar contract invoke --id $TANDA_ID --source alice --network testnet \
  -- initialize \
  --admin $(stellar keys public-key alice) \
  --max_participants 5 \
  --payment_amount 1000000000 \
  --period_secs 2592000 \
  --payment_token $USDC_ID \
  --cetes_token $EF_ID

# Query state
stellar contract invoke --id $TANDA_ID --network testnet -- get_config
stellar contract invoke --id $TANDA_ID --network testnet -- get_round_info
stellar contract invoke --id $TANDA_ID --network testnet \
  -- get_participant --participant <ADDRESS>
```

## Security invariants (never break these)
- Every write function calls `address.require_auth()` before mutating state
- `last_paid_round == current_round` → participant already paid (AlreadyPaid)
- `has_received_payout == true` → cannot claim again (AlreadyReceivedPayout)
- Payout only allowed when `payout_round < current_round` OR status == Completed
- `overflow-checks = true` in Cargo release profile — do not disable

## Testing notes
- All tests use `env.mock_all_auths()` to skip auth in unit tests
- MockEtherfuse is defined in `test.rs` — it stores the token address and
  actually transfers USDC on `redeem()` (1:1, no yield in mock)
- Use `advance_time(&env, secs)` helper to move past payment windows
- Test snapshot files in `test_snapshots/` are auto-generated by Soroban testutils
