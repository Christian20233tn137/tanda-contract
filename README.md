# Tanda — On-Chain Group Savings Contract (Soroban / Stellar)

A fully transparent, automated **rotating savings and credit association (ROSCA)**
built with Soroban smart contracts on the Stellar blockchain.

---

## What is a Tanda?

A *tanda* (or *cundina*) is a traditional Latin American group savings mechanism:

1. N people agree to each contribute a fixed amount every period.
2. Each period the entire pot goes to one member (in a predetermined rotation).
3. After N periods everyone has received the pot exactly once.

This contract automates the tanda entirely on-chain, adds CETES yield via
**Etherfuse stablebonds**, and provides a 10% collateral buffer to handle
missed payments.

---

## Economics Per Payment

```
Payment = 1 000 USDC
├── 10%  → 100 USDC held as personal collateral  (stays in contract)
└── 90%  → 900 USDC invested in Etherfuse CETES  (earns yield)
```

When the beneficiary's turn arrives they receive:
- **Principal**: 90% × N × payment amount (redeemed from CETES)
- **Yield**: CETES appreciation since the round's tokens were minted
- **Collateral return** (end of tanda only): their accumulated 10% holdings

---

## Architecture

```
contracts/tanda/src/
├── lib.rs          Main contract — all callable functions
├── types.rs        TandaConfig, ParticipantInfo, RoundInfo, InvestmentPool
├── errors.rs       TandaError enum (contracterror)
├── events.rs       On-chain event emitters for full auditability
└── etherfuse.rs    Cross-contract client for Etherfuse stablebond
```

---

## Contract Functions

### Setup
| Function | Who calls | Description |
|---|---|---|
| `initialize(admin, max_participants, payment_amount, period_secs, payment_token, cetes_token)` | Deployer | Create the tanda |
| `register(participant)` | Each participant | Join (proof-of-funds checked); auto-starts when full |

### Payments
| Function | Who calls | Description |
|---|---|---|
| `make_payment(participant)` | Participant | Pay current round; 10% collateral, 90% to CETES |
| `handle_missed_payment(missed)` | Anyone | After window closes, cover missed payment from collateral |
| `finalize_round(caller)` | Admin or participant | Advance to next round once all paid |

### Payouts
| Function | Who calls | Description |
|---|---|---|
| `claim_payout(participant)` | Participant | Redeem CETES + receive USDC when it's your turn |
| `reinvest_payout(participant)` | Participant | Signal intent to keep funds in CETES (auditable event) |

### View Functions (free, no auth required)
| Function | Returns |
|---|---|
| `get_config()` | TandaConfig |
| `get_round_info()` | Current RoundInfo |
| `get_investment_pool()` | InvestmentPool |
| `get_collateral_pool()` | i128 (USDC) |
| `get_participant(address)` | ParticipantInfo |
| `get_participants()` | Vec<Address> |
| `get_turn_order()` | Vec<Address> |
| `get_round_cetes(round)` | i128 (CETES tokens for that round) |

---

## On-Chain Events

All state changes emit structured events for easy auditing:

| Event | Topics | Data |
|---|---|---|
| `registered` | `("registered", participant)` | `turn` |
| `tanda_start` | `("tanda_start",)` | `(start_time, max_participants)` |
| `paid` | `("paid", participant)` | `(round, amount, collateral, invested, cetes_minted)` |
| `missed` | `("missed", participant)` | `(round, own_collateral_used, pool_used)` |
| `round_end` | `("round_end",)` | `(round, beneficiary)` |
| `claimed` | `("claimed", participant)` | `(round, principal, yield, collateral_returned)` |
| `reinvested` | `("reinvested", participant)` | `(round, cetes_kept)` |

---

## Security

| Risk | Mitigation |
|---|---|
| **Reentrancy** | Soroban's synchronous host model — no reentrancy possible |
| **Integer overflow** | `overflow-checks = true` in release profile; `i128` arithmetic |
| **Unauthorized access** | `address.require_auth()` on every write function |
| **Double payment** | `last_paid_round` sentinel prevents paying twice per round |
| **Double claim** | `has_received_payout` flag |
| **Early claim** | Turn eligibility gated on `current_round > payout_round` |
| **Sybil registration** | Proof-of-funds: must hold ≥ 2× payment amount of USDC |

---

## Etherfuse Integration

[Etherfuse](https://etherfuse.com) tokenises Mexican CETES government bonds on Stellar.
Their stablebond protocol:

1. Accepts USDC → mints CETES tokens at current NAV.
2. CETES tokens appreciate daily as the underlying bond earns interest.
3. Redeeming CETES returns USDC + accrued yield.

Pass the Etherfuse stablebond contract address as `cetes_token` in `initialize()`.

---

## Running Tests

```bash
cargo test
```

12 tests cover: initialization, registration, auto-start, proof-of-funds gate,
payments, double-payment prevention, missed payments with collateral coverage,
full 3-round lifecycle, window enforcement, and payout eligibility gating.

---

## Build WASM

```bash
stellar contract build
# Output: target/wasm32v1-none/release/tanda.wasm
```
