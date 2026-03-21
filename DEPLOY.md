# Deployment Guide — Tanda Contract

---

## Prerequisites

```bash
# Rust + wasm target
rustup target add wasm32v1-none

# Stellar CLI (already installed)
stellar --version
```

---

## 1. Configure Network Identity

```bash
# Generate or reuse your deployer key (already done — "alice")
stellar keys generate alice --network testnet

# Fund on testnet
stellar keys fund alice --network testnet
```

---

## 2. Build the WASM

```bash
cd tanda-contract
stellar contract build
# → target/wasm32v1-none/release/tanda.wasm
```

---

## 3. Deploy to Testnet

```bash
stellar contract deploy \
  --wasm target/wasm32v1-none/release/tanda.wasm \
  --source alice \
  --network testnet
# Prints: CONTRACT_ID (save this)
```

Set a shell variable for convenience:
```bash
export TANDA_ID=<CONTRACT_ID printed above>
```

---

## 4. Obtain Token Addresses

**USDC on Stellar testnet** — deploy the official Circle USDC or a test token:
```bash
stellar contract asset deploy \
  --asset USDC:GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5 \
  --source alice --network testnet
# Prints USDC contract ID → save as USDC_ID
```

**Etherfuse stablebond** — on testnet use the Etherfuse test contract address
(check https://docs.etherfuse.com for current testnet IDs), or deploy the mock:
```bash
stellar contract deploy \
  --wasm path/to/mock_etherfuse.wasm \
  --source alice \
  --network testnet
# → save as EF_ID
```

---

## 5. Initialize the Tanda

```bash
# Example: 5 participants, 1000 USDC/month, 30-day windows
stellar contract invoke \
  --id $TANDA_ID \
  --source alice \
  --network testnet \
  -- initialize \
  --admin $(stellar keys public-key alice) \
  --max_participants 5 \
  --payment_amount 1000000000 \
  --period_secs 2592000 \
  --payment_token $USDC_ID \
  --cetes_token $EF_ID
```

---

## 6. Participant Registration

Each participant runs this from their own key:
```bash
stellar contract invoke \
  --id $TANDA_ID \
  --source <participant-key> \
  --network testnet \
  -- register \
  --participant $(stellar keys public-key <participant-key>)
```

The tanda starts automatically when all slots are filled.

---

## 7. Making Payments

```bash
# Each participant runs each month:
stellar contract invoke \
  --id $TANDA_ID \
  --source <participant-key> \
  --network testnet \
  -- make_payment \
  --participant $(stellar keys public-key <participant-key>)
```

---

## 8. Finalize a Round

After all participants have paid (or missed payments handled):
```bash
stellar contract invoke \
  --id $TANDA_ID \
  --source alice \
  --network testnet \
  -- finalize_round \
  --caller $(stellar keys public-key alice)
```

---

## 9. Claim Payout

When it's your turn:
```bash
stellar contract invoke \
  --id $TANDA_ID \
  --source <participant-key> \
  --network testnet \
  -- claim_payout \
  --participant $(stellar keys public-key <participant-key>)
```

---

## 10. Query State

```bash
# Tanda status
stellar contract invoke --id $TANDA_ID --network testnet -- get_config

# Your info
stellar contract invoke --id $TANDA_ID --network testnet \
  -- get_participant --participant <ADDRESS>

# Current round
stellar contract invoke --id $TANDA_ID --network testnet -- get_round_info

# Collateral pool
stellar contract invoke --id $TANDA_ID --network testnet -- get_collateral_pool
```

---

## Mainnet Deployment

Replace `--network testnet` with `--network mainnet` in all commands.
Use the official Etherfuse stablebond contract address for mainnet CETES.

> **Audit recommendation**: Have the contract independently audited before
> mainnet deployment with real funds.
