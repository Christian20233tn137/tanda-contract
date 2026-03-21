/// On-chain event helpers.
///
/// Every state-changing action emits a structured event so that off-chain
/// auditors (wallets, dashboards, explorers) can reconstruct the full history
/// of the tanda without reading contract storage.
use soroban_sdk::{Address, Env, Symbol};

// ── topic symbols (≤9 chars each) ──────────────────────────────────────────

fn sym(env: &Env, s: &str) -> Symbol {
    Symbol::new(env, s)
}

// ── emitters ───────────────────────────────────────────────────────────────

/// Participant successfully registered.
pub fn emit_registered(env: &Env, participant: &Address, turn: u32) {
    env.events()
        .publish((sym(env, "registered"), participant.clone()), turn);
}

/// Tanda moved to Active status.
pub fn emit_tanda_started(env: &Env, start_time: u64, max_participants: u32) {
    env.events()
        .publish((sym(env, "tanda_start"),), (start_time, max_participants));
}

/// A payment was successfully processed.
/// `collateral` = USDC retained (10%), `invested` = USDC sent to Etherfuse.
pub fn emit_payment_made(
    env: &Env,
    participant: &Address,
    round: u32,
    amount: i128,
    collateral: i128,
    invested: i128,
    cetes_minted: i128,
) {
    env.events().publish(
        (sym(env, "paid"), participant.clone()),
        (round, amount, collateral, invested, cetes_minted),
    );
}

/// A participant missed their payment; collateral was used to cover.
pub fn emit_payment_missed(
    env: &Env,
    participant: &Address,
    round: u32,
    own_collateral_used: i128,
    pool_used: i128,
) {
    env.events().publish(
        (sym(env, "missed"), participant.clone()),
        (round, own_collateral_used, pool_used),
    );
}

/// A round was finalised and the next round started (or tanda completed).
pub fn emit_round_finalized(env: &Env, round: u32, beneficiary: &Address) {
    env.events()
        .publish((sym(env, "round_end"),), (round, beneficiary.clone()));
}

/// Beneficiary withdrew their payout from the tanda.
pub fn emit_payout_claimed(
    env: &Env,
    participant: &Address,
    payout_round: u32,
    principal: i128,
    yield_amount: i128,
    collateral_returned: i128,
) {
    env.events().publish(
        (sym(env, "claimed"), participant.clone()),
        (payout_round, principal, yield_amount, collateral_returned),
    );
}

/// Beneficiary elected to leave their payout invested in CETES.
pub fn emit_payout_reinvested(env: &Env, participant: &Address, payout_round: u32, cetes_kept: i128) {
    env.events().publish(
        (sym(env, "reinvested"), participant.clone()),
        (payout_round, cetes_kept),
    );
}
