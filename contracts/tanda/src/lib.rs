//! # Tanda — Group Savings Smart Contract (Soroban / Stellar)
//!
//! A fully on-chain rotating savings and credit association (ROSCA / "tanda").
//!
//! ## Lifecycle
//! ```
//! deploy (constructor) → register() × N → [auto-start] → make_payment() × (N rounds × N participants)
//!   → finalize_round() → claim_payout() / reinvest_payout()  [× N rounds]
//! ```
//!
//! ## Economics (per payment)
//! | Slice         | Amount  | Destination          |
//! |---------------|---------|----------------------|
//! | Collateral    | 10%     | Contract (USDC)      |
//! | Investment    | 90%     | Etherfuse CETES      |
//!
//! When a participant's turn arrives the contract redeems their round's CETES
//! tokens (principal + yield) and pays them out.  Collateral is returned at
//! the end of the tanda, unless used to cover a missed payment.

#![no_std]

mod errors;
mod etherfuse;
mod events;
mod types;

use errors::TandaError;
use etherfuse::EtherfuseClient;
use events::*;
use types::*;

use soroban_sdk::{
    contract, contractimpl, contractmeta, contracttype, Address, BytesN, Env, Vec,
    token::Client as TokenClient,
};

// ─── Contract metadata (SEP-49 / on-chain version tracking) ──────────────────

contractmeta!(key = "Description", val = "Tanda ROSCA Group Savings — Soroban");
contractmeta!(key = "binver", val = "1.0.0");

// ─── Storage keys ─────────────────────────────────────────────────────────────

#[contracttype]
enum DataKey {
    Config,
    /// Ordered registration list (Vec<Address>).
    Participants,
    /// Payout order — same as Participants but explicit.
    TurnOrder,
    /// Per-participant mutable state.
    Participant(Address),
    /// Current round state.
    RoundInfo,
    /// CETES tokens accumulated during round N.
    RoundCetes(u32),
    /// Shared CETES investment pool totals.
    InvestmentPool,
    /// Shared USDC collateral pool (held as raw USDC in the contract).
    CollateralPool,
}

// ─── TTL constants (Stellar mainnet ~5 s / ledger) ────────────────────────────

const TTL_BUMP: u32 = 535_000;       // ≈ 1 year
const TTL_THRESHOLD: u32 = 435_000;

// ─── Arithmetic helpers ───────────────────────────────────────────────────────

const BPS_DENOM: i128 = 10_000;

fn bps_of(amount: i128, bps: u32) -> i128 {
    amount * bps as i128 / BPS_DENOM
}

// ─── Storage helpers ──────────────────────────────────────────────────────────

fn load_cfg(env: &Env) -> TandaConfig {
    env.storage().instance().get(&DataKey::Config).unwrap()
}
fn store_cfg(env: &Env, c: &TandaConfig) {
    env.storage().instance().set(&DataKey::Config, c);
}

fn load_participants(env: &Env) -> Vec<Address> {
    env.storage()
        .instance()
        .get(&DataKey::Participants)
        .unwrap_or(Vec::new(env))
}
fn store_participants(env: &Env, v: &Vec<Address>) {
    env.storage().instance().set(&DataKey::Participants, v);
}

fn load_turn_order(env: &Env) -> Vec<Address> {
    env.storage()
        .instance()
        .get(&DataKey::TurnOrder)
        .unwrap_or(Vec::new(env))
}
fn store_turn_order(env: &Env, v: &Vec<Address>) {
    env.storage().instance().set(&DataKey::TurnOrder, v);
}

fn load_participant(env: &Env, addr: &Address) -> Option<ParticipantInfo> {
    env.storage()
        .persistent()
        .get(&DataKey::Participant(addr.clone()))
}
fn store_participant(env: &Env, addr: &Address, p: &ParticipantInfo) {
    let key = DataKey::Participant(addr.clone());
    env.storage().persistent().set(&key, p);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_BUMP);
}

fn load_round(env: &Env) -> RoundInfo {
    env.storage().instance().get(&DataKey::RoundInfo).unwrap()
}
fn store_round(env: &Env, r: &RoundInfo) {
    env.storage().instance().set(&DataKey::RoundInfo, r);
}

fn load_round_cetes(env: &Env, round: u32) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::RoundCetes(round))
        .unwrap_or(0i128)
}
fn accum_round_cetes(env: &Env, round: u32, amount: i128) {
    let key = DataKey::RoundCetes(round);
    let prev: i128 = env.storage().persistent().get(&key).unwrap_or(0);
    env.storage().persistent().set(&key, &(prev + amount));
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_BUMP);
}

fn load_investment_pool(env: &Env) -> InvestmentPool {
    env.storage()
        .instance()
        .get(&DataKey::InvestmentPool)
        .unwrap_or(InvestmentPool {
            total_cetes_tokens: 0,
            total_usdc_invested: 0,
            accumulated_yield: 0,
        })
}
fn store_investment_pool(env: &Env, p: &InvestmentPool) {
    env.storage().instance().set(&DataKey::InvestmentPool, p);
}

fn load_collateral_pool(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::CollateralPool)
        .unwrap_or(0i128)
}
fn store_collateral_pool(env: &Env, amount: i128) {
    env.storage()
        .instance()
        .set(&DataKey::CollateralPool, &amount);
}

fn bump_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(TTL_THRESHOLD, TTL_BUMP);
}

// ─── Payout turn helper ───────────────────────────────────────────────────────

/// Look up the payout round index for `participant` in the turn order.
fn find_payout_round(env: &Env, participant: &Address) -> Result<u32, TandaError> {
    let order = load_turn_order(env);
    for i in 0..order.len() {
        if order.get(i).unwrap() == *participant {
            return Ok(i);
        }
    }
    Err(TandaError::ParticipantNotFound)
}

// ─── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct TandaContract;

#[contractimpl]
impl TandaContract {
    // ═══════════════════════════════════════════════════════════════════════
    // SETUP
    // ═══════════════════════════════════════════════════════════════════════

    /// Initialise a new tanda at deployment time (Protocol 22 constructor).
    ///
    /// Runs exactly once — atomically with contract creation.
    ///
    /// # Arguments
    /// * `admin`            – Administrator (can finalize rounds after window closes).
    /// * `max_participants` – 2–20 inclusive.
    /// * `payment_amount`   – Fixed periodic payment in USDC micro-units (6 dp).
    ///   e.g. `1_000_000_000` = 1 000 USDC.
    /// * `period_secs`      – Payment window per round (e.g. `2_592_000` = 30 days).
    /// * `payment_token`    – USDC SEP-41 contract address.
    /// * `cetes_token`      – Etherfuse stablebond contract address.
    pub fn __constructor(
        env: Env,
        admin: Address,
        max_participants: u32,
        payment_amount: i128,
        period_secs: u64,
        payment_token: Address,
        cetes_token: Address,
    ) {
        if !(2..=20).contains(&max_participants) || payment_amount <= 0 || period_secs == 0 {
            panic!("invalid params");
        }
        admin.require_auth();

        store_cfg(
            &env,
            &TandaConfig {
                admin,
                max_participants,
                payment_amount,
                period_secs,
                payment_token,
                cetes_token,
                collateral_bps: 1_000, // 10 %
                status: TandaStatus::Registering,
                start_time: 0,
                current_round: 0,
                total_rounds: max_participants,
            },
        );
        bump_instance(&env);
    }

    /// Replace the contract's WASM binary (admin-only).
    ///
    /// The new implementation takes effect after this invocation completes.
    /// Storage keys and types must remain compatible — see upgrade-safety notes.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let c = load_cfg(&env);
        c.admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // REGISTRATION
    // ═══════════════════════════════════════════════════════════════════════

    /// Register as a tanda participant.
    ///
    /// - Requires `Registering` status.
    /// - Caller must hold ≥ `2 × payment_amount` USDC (proof-of-funds gate).
    /// - When the last slot is filled the tanda auto-transitions to `Active`.
    pub fn register(env: Env, participant: Address) -> Result<(), TandaError> {
        participant.require_auth();

        let mut c = load_cfg(&env);
        if c.status != TandaStatus::Registering {
            return Err(TandaError::TandaNotRegistering);
        }
        if load_participant(&env, &participant).is_some() {
            return Err(TandaError::AlreadyRegistered);
        }

        let mut list = load_participants(&env);
        if list.len() >= c.max_participants {
            return Err(TandaError::TandaFull);
        }

        // Proof-of-funds: participant must hold at least 2× payment to register
        let token = TokenClient::new(&env, &c.payment_token);
        if token.balance(&participant) < c.payment_amount * 2 {
            return Err(TandaError::InsufficientBalance);
        }

        let turn = list.len();
        list.push_back(participant.clone());
        store_participants(&env, &list);

        store_participant(
            &env,
            &participant,
            &ParticipantInfo {
                turn,
                total_paid: 0,
                collateral_held: 0,
                last_paid_round: u32::MAX, // sentinel = never paid
                has_received_payout: false,
                missed_payments: 0,
            },
        );

        RegisteredEvent { participant: participant.clone(), turn }.publish(&env);

        // Auto-start when all slots filled
        if list.len() >= c.max_participants {
            let now = env.ledger().timestamp();
            c.status = TandaStatus::Active;
            c.start_time = now;
            c.current_round = 0;

            store_turn_order(&env, &list);

            store_round(
                &env,
                &RoundInfo {
                    round: 0,
                    start_time: now,
                    beneficiary: list.get(0).unwrap(),
                    payments_received: 0,
                    total_collected: 0,
                    is_finalized: false,
                },
            );

            TandaStartedEvent { start_time: now, max_participants: c.max_participants }.publish(&env);
        }

        store_cfg(&env, &c);
        bump_instance(&env);
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════════════════
    // PAYMENTS
    // ═══════════════════════════════════════════════════════════════════════

    /// Make the periodic payment for the current round.
    ///
    /// Economics:
    /// - 10% retained as personal collateral (USDC stays in contract).
    /// - 90% forwarded to Etherfuse; CETES tokens are minted to the contract
    ///   and credited to this round's pool.
    pub fn make_payment(env: Env, participant: Address) -> Result<(), TandaError> {
        participant.require_auth();

        let c = load_cfg(&env);
        if c.status != TandaStatus::Active {
            return Err(TandaError::TandaNotActive);
        }

        let mut r = load_round(&env);
        if r.is_finalized {
            return Err(TandaError::RoundAlreadyFinalized);
        }

        let now = env.ledger().timestamp();
        if now > r.start_time + c.period_secs {
            return Err(TandaError::PaymentWindowClosed);
        }

        let mut p = load_participant(&env, &participant)
            .ok_or(TandaError::ParticipantNotFound)?;

        if p.last_paid_round == c.current_round {
            return Err(TandaError::AlreadyPaid);
        }

        // ── Split ──────────────────────────────────────────────────────────
        let collateral = bps_of(c.payment_amount, c.collateral_bps);
        let invest_amount = c.payment_amount - collateral;

        // Pull payment from participant → tanda contract
        let token = TokenClient::new(&env, &c.payment_token);
        token.transfer(&participant, &env.current_contract_address(), &c.payment_amount);

        // Forward invest_amount to Etherfuse contract (USDC transfer)
        token.transfer(
            &env.current_contract_address(),
            &c.cetes_token,
            &invest_amount,
        );

        // Notify Etherfuse to mint CETES tokens back to this contract
        let ef = EtherfuseClient::new(&env, &c.cetes_token);
        let cetes_minted = ef.deposit(&env.current_contract_address(), invest_amount);

        // ── Persist ────────────────────────────────────────────────────────
        accum_round_cetes(&env, c.current_round, cetes_minted);

        let mut pool = load_investment_pool(&env);
        pool.total_cetes_tokens += cetes_minted;
        pool.total_usdc_invested += invest_amount;
        store_investment_pool(&env, &pool);

        store_collateral_pool(&env, load_collateral_pool(&env) + collateral);

        p.total_paid += c.payment_amount;
        p.collateral_held += collateral;
        p.last_paid_round = c.current_round;
        store_participant(&env, &participant, &p);

        r.payments_received += 1;
        r.total_collected += c.payment_amount;
        store_round(&env, &r);

        PaymentMadeEvent {
            participant: participant.clone(),
            round: c.current_round,
            amount: c.payment_amount,
            collateral,
            invested: invest_amount,
            cetes_minted,
        }.publish(&env);
        bump_instance(&env);
        Ok(())
    }

    /// Cover a missed payment using the participant's held collateral (then shared pool).
    ///
    /// Can be called by anyone after the payment window for the current round expires.
    pub fn handle_missed_payment(
        env: Env,
        missed_participant: Address,
    ) -> Result<(), TandaError> {
        let c = load_cfg(&env);
        if c.status != TandaStatus::Active {
            return Err(TandaError::TandaNotActive);
        }

        let r = load_round(&env);
        if r.is_finalized {
            return Err(TandaError::RoundAlreadyFinalized);
        }

        let now = env.ledger().timestamp();
        if now <= r.start_time + c.period_secs {
            return Err(TandaError::PaymentWindowOpen); // window still open
        }

        let mut p = load_participant(&env, &missed_participant)
            .ok_or(TandaError::ParticipantNotFound)?;

        if p.last_paid_round == c.current_round {
            return Err(TandaError::AlreadyPaid); // they paid
        }

        // ── Coverage ───────────────────────────────────────────────────────
        let own_coverage = p.collateral_held.min(c.payment_amount);
        p.collateral_held -= own_coverage;

        let shortfall = c.payment_amount - own_coverage;
        let mut cp = load_collateral_pool(&env);
        // Draw as much as available from the shared pool; accept partial cover.
        // Any remainder is absorbed as a smaller round payout for the beneficiary.
        let pool_coverage = if shortfall > 0 {
            let available = cp.min(shortfall);
            cp -= available;
            available
        } else {
            0
        };

        p.missed_payments += 1;
        p.last_paid_round = c.current_round; // mark handled
        store_participant(&env, &missed_participant, &p);
        store_collateral_pool(&env, cp);

        let mut r_new = r;
        r_new.payments_received += 1;
        // Only credit what was actually covered, not the full payment_amount.
        // Any shortfall is absorbed as a smaller payout for the beneficiary.
        r_new.total_collected += own_coverage + pool_coverage;
        store_round(&env, &r_new);

        let actual_shortfall = c.payment_amount - own_coverage - pool_coverage;
        PaymentMissedEvent {
            participant: missed_participant.clone(),
            round: c.current_round,
            own_collateral_used: own_coverage,
            pool_used: pool_coverage,
            shortfall: actual_shortfall,
        }.publish(&env);
        bump_instance(&env);
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════════════════
    // ROUND MANAGEMENT
    // ═══════════════════════════════════════════════════════════════════════

    /// Finalise the current round and begin the next (or complete the tanda).
    ///
    /// Requirements: all participants must have paid or been covered.
    /// Can be called by admin or any registered participant.
    pub fn finalize_round(env: Env, caller: Address) -> Result<(), TandaError> {
        caller.require_auth();

        let mut c = load_cfg(&env);
        if c.status != TandaStatus::Active {
            return Err(TandaError::TandaNotActive);
        }

        let mut r = load_round(&env);
        if r.is_finalized {
            return Err(TandaError::RoundAlreadyFinalized);
        }
        if r.payments_received < c.max_participants {
            return Err(TandaError::RoundNotFinalized);
        }

        r.is_finalized = true;
        store_round(&env, &r);
        RoundFinalizedEvent { round: c.current_round, beneficiary: r.beneficiary.clone() }.publish(&env);

        c.current_round += 1;

        if c.current_round >= c.total_rounds {
            c.status = TandaStatus::Completed;
        } else {
            let to = load_turn_order(&env);
            store_round(
                &env,
                &RoundInfo {
                    round: c.current_round,
                    start_time: env.ledger().timestamp(),
                    beneficiary: to.get(c.current_round).unwrap(),
                    payments_received: 0,
                    total_collected: 0,
                    is_finalized: false,
                },
            );
        }

        store_cfg(&env, &c);
        bump_instance(&env);
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════════════════
    // PAYOUTS
    // ═══════════════════════════════════════════════════════════════════════

    /// Withdraw the payout when it's the participant's turn.
    ///
    /// Redeems the round's CETES tokens from Etherfuse and transfers USDC +
    /// yield to the participant.  If the tanda is `Completed` any remaining
    /// personal collateral is also returned.
    pub fn claim_payout(env: Env, participant: Address) -> Result<i128, TandaError> {
        participant.require_auth();

        let c = load_cfg(&env);
        let payout_round = find_payout_round(&env, &participant)?;

        let eligible =
            payout_round < c.current_round || c.status == TandaStatus::Completed;
        if !eligible {
            return Err(TandaError::NotYourTurn);
        }

        let mut p = load_participant(&env, &participant)
            .ok_or(TandaError::ParticipantNotFound)?;
        if p.has_received_payout {
            return Err(TandaError::AlreadyReceivedPayout);
        }

        // ── Redeem CETES ───────────────────────────────────────────────────
        let cetes_balance = load_round_cetes(&env, payout_round);
        let token = TokenClient::new(&env, &c.payment_token);
        let ef = EtherfuseClient::new(&env, &c.cetes_token);

        let invest_per_payment = c.payment_amount - bps_of(c.payment_amount, c.collateral_bps);
        let principal = invest_per_payment * c.max_participants as i128;

        let usdc_from_cetes = if cetes_balance > 0 {
            ef.redeem(&env.current_contract_address(), cetes_balance)
        } else {
            0
        };

        let yield_amount = if usdc_from_cetes > principal {
            usdc_from_cetes - principal
        } else {
            0
        };

        // Return personal collateral when tanda completes
        let collateral_return = if c.status == TandaStatus::Completed {
            p.collateral_held
        } else {
            0
        };

        let total_payout = usdc_from_cetes + collateral_return;
        token.transfer(&env.current_contract_address(), &participant, &total_payout);

        // ── Update pools ───────────────────────────────────────────────────
        let mut pool = load_investment_pool(&env);
        pool.total_cetes_tokens -= cetes_balance;
        pool.accumulated_yield += yield_amount;
        store_investment_pool(&env, &pool);

        // Clean up round CETES storage key (no longer needed after redemption)
        env.storage().persistent().remove(&DataKey::RoundCetes(payout_round));

        if collateral_return > 0 {
            let cp = load_collateral_pool(&env);
            // Cap to available pool to prevent underflow from accounting drift
            let actual_return = collateral_return.min(cp);
            store_collateral_pool(&env, cp - actual_return);
        }

        p.has_received_payout = true;
        if c.status == TandaStatus::Completed {
            p.collateral_held = 0;
        }
        store_participant(&env, &participant, &p);

        PayoutClaimedEvent {
            participant: participant.clone(),
            payout_round,
            principal,
            yield_amount,
            collateral_returned: collateral_return,
        }.publish(&env);
        bump_instance(&env);
        Ok(total_payout)
    }

    /// Signal intent to keep the round's CETES tokens invested.
    ///
    /// No funds move; the participant can still call `claim_payout` later.
    /// Emits an auditable on-chain event.
    pub fn reinvest_payout(env: Env, participant: Address) -> Result<(), TandaError> {
        participant.require_auth();

        let c = load_cfg(&env);
        let payout_round = find_payout_round(&env, &participant)?;

        if payout_round >= c.current_round && c.status != TandaStatus::Completed {
            return Err(TandaError::NotYourTurn);
        }

        let p = load_participant(&env, &participant)
            .ok_or(TandaError::ParticipantNotFound)?;
        if p.has_received_payout {
            return Err(TandaError::AlreadyReceivedPayout);
        }

        let cetes_kept = load_round_cetes(&env, payout_round);
        if cetes_kept == 0 {
            return Err(TandaError::NoCetesToReinvest);
        }

        // Mark as received so participant cannot also call claim_payout
        let mut p_updated = p;
        p_updated.has_received_payout = true;
        store_participant(&env, &participant, &p_updated);

        PayoutReinvestedEvent { participant: participant.clone(), payout_round, cetes_kept }.publish(&env);
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════════════════
    // VIEW FUNCTIONS (read-only, no auth required)
    // ═══════════════════════════════════════════════════════════════════════

    pub fn get_config(env: Env) -> TandaConfig {
        load_cfg(&env)
    }

    pub fn get_round_info(env: Env) -> RoundInfo {
        load_round(&env)
    }

    pub fn get_investment_pool(env: Env) -> InvestmentPool {
        load_investment_pool(&env)
    }

    pub fn get_collateral_pool(env: Env) -> i128 {
        load_collateral_pool(&env)
    }

    pub fn get_participant(env: Env, participant: Address) -> Result<ParticipantInfo, TandaError> {
        load_participant(&env, &participant).ok_or(TandaError::ParticipantNotFound)
    }

    pub fn get_participants(env: Env) -> Vec<Address> {
        load_participants(&env)
    }

    pub fn get_turn_order(env: Env) -> Vec<Address> {
        load_turn_order(&env)
    }

    pub fn get_round_cetes(env: Env, round: u32) -> i128 {
        load_round_cetes(&env, round)
    }
}

#[cfg(test)]
mod test;
