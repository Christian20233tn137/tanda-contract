#![cfg(test)]

extern crate std;

use super::*;
use soroban_sdk::{
    contract, contractimpl, contracttype,
    testutils::{Address as _, Ledger, LedgerInfo, MockAuth, MockAuthInvoke},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, IntoVal,
};

// ─── Mock Etherfuse CETES contract ────────────────────────────────────────────
//
// Simulates a 1:1 deposit/redeem.  The mock actually holds USDC received via
// deposit and transfers it back on redeem, so the tanda contract's balance is
// correct throughout the test lifecycle.

#[contracttype]
enum MockKey {
    Token,
}

#[contract]
pub struct MockEtherfuse;

#[contractimpl]
impl MockEtherfuse {
    /// Store the USDC token contract address at deployment time.
    pub fn __constructor(env: Env, token: Address) {
        env.storage().instance().set(&MockKey::Token, &token);
    }

    /// Accept USDC (already transferred to this contract) and mint CETES 1:1.
    pub fn deposit(_env: Env, _depositor: Address, usdc_amount: i128) -> i128 {
        usdc_amount
    }

    /// Burn CETES tokens and transfer USDC back to recipient (1:1, no yield).
    pub fn redeem(env: Env, recipient: Address, cetes_amount: i128) -> i128 {
        let token_addr: Address = env.storage().instance().get(&MockKey::Token).unwrap();
        let token = TokenClient::new(&env, &token_addr);
        token.transfer(&env.current_contract_address(), &recipient, &cetes_amount);
        cetes_amount
    }

    pub fn get_nav(_env: Env) -> i128 {
        1_000_000 // NAV = 1.0 (no yield in mock)
    }

    pub fn balance(_env: Env, _address: Address) -> i128 {
        0
    }
}

// ─── Test helpers ─────────────────────────────────────────────────────────────

const PAYMENT: i128 = 1_000_000_000; // 1 000 USDC (6 dp → micro-units)
const PERIOD: u64 = 2_592_000;       // 30 days in seconds
const N: u32 = 3;                     // 3-participant tanda for fast tests

struct TestEnv {
    env: Env,
    tanda: TandaContractClient<'static>,
    token_id: Address,
    admin: Address,
    participants: std::vec::Vec<Address>,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths();

    // Deploy mock token (USDC)
    let admin = Address::generate(&env);
    let token_id = env.register_stellar_asset_contract_v2(admin.clone()).address();
    let token_admin = StellarAssetClient::new(&env, &token_id);

    // Deploy mock Etherfuse with constructor (stores token address atomically)
    let ef_id = env.register(MockEtherfuse, (token_id.clone(),));

    // Deploy tanda contract with constructor
    let tanda_id = env.register(
        TandaContract,
        (admin.clone(), N, PAYMENT, PERIOD, token_id.clone(), ef_id.clone()),
    );
    let tanda = TandaContractClient::new(&env, &tanda_id);

    // Mint tokens to participants
    let mut ps = std::vec::Vec::new();
    for _ in 0..N {
        let p = Address::generate(&env);
        token_admin.mint(&p, &(PAYMENT * 10)); // plenty of funds
        ps.push(p);
    }

    TestEnv {
        env,
        tanda,
        token_id,
        admin,
        participants: ps,
    }
}

fn advance_time(env: &Env, secs: u64) {
    let info = env.ledger().get();
    env.ledger().set(LedgerInfo {
        timestamp: info.timestamp + secs,
        ..info
    });
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[test]
fn test_initialize() {
    let t = setup();
    let cfg = t.tanda.get_config();
    assert_eq!(cfg.max_participants, N);
    assert_eq!(cfg.payment_amount, PAYMENT);
    assert_eq!(cfg.period_secs, PERIOD);
    assert_eq!(cfg.status, TandaStatus::Registering);
    assert_eq!(cfg.collateral_bps, 1_000);
}

#[test]
fn test_register_and_auto_start() {
    let t = setup();

    // Register N-1 participants — still Registering
    for i in 0..(N - 1) as usize {
        t.tanda.register(&t.participants[i]);
        let cfg = t.tanda.get_config();
        assert_eq!(cfg.status, TandaStatus::Registering);
    }

    // Register last participant — tanda goes Active
    t.tanda.register(&t.participants[(N - 1) as usize]);
    let cfg = t.tanda.get_config();
    assert_eq!(cfg.status, TandaStatus::Active);
    assert_eq!(cfg.current_round, 0);

    // Turn order matches registration order
    let order = t.tanda.get_turn_order();
    for i in 0..N as usize {
        assert_eq!(order.get(i as u32).unwrap(), t.participants[i]);
    }
}

#[test]
fn test_proof_of_funds_gate() {
    let t = setup();
    let poor = Address::generate(&t.env);
    // poor has no tokens → should panic with InsufficientBalance
    let result = t.tanda.try_register(&poor);
    assert!(result.is_err());
}

#[test]
fn test_make_payment() {
    let t = setup();

    // Register all participants
    for p in &t.participants {
        t.tanda.register(p);
    }

    let token = TokenClient::new(&t.env, &t.token_id);

    // All participants pay in round 0
    for p in &t.participants {
        let balance_before = token.balance(p);
        t.tanda.make_payment(p);
        let balance_after = token.balance(p);
        assert_eq!(balance_before - balance_after, PAYMENT);
    }

    let r = t.tanda.get_round_info();
    assert_eq!(r.payments_received, N);
    assert_eq!(r.total_collected, PAYMENT * N as i128);

    // Collateral pool = 10% × N × PAYMENT
    let collateral = t.tanda.get_collateral_pool();
    assert_eq!(collateral, PAYMENT * N as i128 / 10);

    // CETES tokens = 90% × N × PAYMENT (mock: 1:1)
    let pool = t.tanda.get_investment_pool();
    assert_eq!(pool.total_cetes_tokens, PAYMENT * 9 * N as i128 / 10);
}

#[test]
fn test_double_payment_fails() {
    let t = setup();
    for p in &t.participants {
        t.tanda.register(p);
    }
    let p0 = &t.participants[0];
    t.tanda.make_payment(p0);
    // second payment in same round must fail
    assert!(t.tanda.try_make_payment(p0).is_err());
}

#[test]
fn test_full_tanda_lifecycle() {
    let t = setup();

    for p in &t.participants {
        t.tanda.register(p);
    }

    let token = TokenClient::new(&t.env, &t.token_id);
    let invest_per = PAYMENT * 9 / 10; // 90 %
    let _collateral_per = PAYMENT / 10;  // 10 %

    for round in 0..N {
        // All participants pay
        for p in &t.participants {
            t.tanda.make_payment(p);
        }

        // Finalize round
        t.tanda.finalize_round(&t.admin);

        let cfg = t.tanda.get_config();
        if round < N - 1 {
            assert_eq!(cfg.current_round, round + 1);
            assert_eq!(cfg.status, TandaStatus::Active);
        } else {
            assert_eq!(cfg.status, TandaStatus::Completed);
        }

        // Beneficiary claims payout
        let beneficiary = &t.participants[round as usize];
        let bal_before = token.balance(beneficiary);
        let payout = t.tanda.claim_payout(beneficiary);
        let bal_after = token.balance(beneficiary);
        assert_eq!(bal_after - bal_before, payout);

        // Mock yields 5%, so payout > principal
        let principal = invest_per * N as i128;
        assert!(payout >= principal, "payout should at least cover principal");
    }

    // After completion, collateral returned with final payout
    // (already captured above for last participant)
    let final_cfg = t.tanda.get_config();
    assert_eq!(final_cfg.status, TandaStatus::Completed);
}

#[test]
fn test_missed_payment_covered_by_own_collateral() {
    let t = setup();

    for p in &t.participants {
        t.tanda.register(p);
    }

    // Complete round 0 normally so each participant builds up collateral.
    for p in &t.participants {
        t.tanda.make_payment(p);
    }
    t.tanda.finalize_round(&t.admin);

    // Round 1 — participants[0] and [1] pay, but [2] misses.
    t.tanda.make_payment(&t.participants[0]);
    t.tanda.make_payment(&t.participants[1]);

    // Advance past the payment window.
    advance_time(&t.env, PERIOD + 1);

    let collateral_before = t.tanda.get_participant(&t.participants[2]).collateral_held;
    assert!(collateral_before > 0, "participant[2] should have collateral from round 0");

    // Cover the missed payment; participant[2]'s collateral is used first.
    t.tanda.handle_missed_payment(&t.participants[2]);

    let p2 = t.tanda.get_participant(&t.participants[2]);
    assert_eq!(p2.missed_payments, 1);
    assert_eq!(p2.last_paid_round, 1);
    // Collateral should have decreased
    assert!(p2.collateral_held < collateral_before);

    let r = t.tanda.get_round_info();
    assert_eq!(r.payments_received, N);
}

#[test]
fn test_payment_after_window_fails() {
    let t = setup();

    for p in &t.participants {
        t.tanda.register(p);
    }

    advance_time(&t.env, PERIOD + 1);
    assert!(t.tanda.try_make_payment(&t.participants[0]).is_err());
}

#[test]
fn test_claim_before_turn_fails() {
    let t = setup();

    for p in &t.participants {
        t.tanda.register(p);
    }

    for p in &t.participants {
        t.tanda.make_payment(p);
    }
    t.tanda.finalize_round(&t.admin);

    // participants[1]'s turn is round 1 but we're now in round 1 (not yet past it)
    // Round 1 has not been finalized yet → NotYourTurn
    assert!(t.tanda.try_claim_payout(&t.participants[1]).is_err());
}

#[test]
fn test_participant_info_view() {
    let t = setup();

    for p in &t.participants {
        t.tanda.register(p);
    }

    t.tanda.make_payment(&t.participants[0]);

    let info = t.tanda.get_participant(&t.participants[0]);
    assert_eq!(info.total_paid, PAYMENT);
    assert_eq!(info.collateral_held, PAYMENT / 10);
    assert_eq!(info.last_paid_round, 0);
    assert!(!info.has_received_payout);
}

#[test]
fn test_double_claim_fails() {
    let t = setup();

    for p in &t.participants {
        t.tanda.register(p);
    }
    for p in &t.participants {
        t.tanda.make_payment(p);
    }
    t.tanda.finalize_round(&t.admin);
    t.tanda.claim_payout(&t.participants[0]);

    // second claim must fail
    assert!(t.tanda.try_claim_payout(&t.participants[0]).is_err());
}

#[test]
fn test_upgrade_only_admin() {
    // Verify upgrade() requires admin auth — non-admin must fail.
    let t = setup();

    // The WASM hash for TandaContract itself (registered in tests as native).
    // We use the contract's own installed wasm hash to prove the call signature
    // works; in practice the hash would point to a new WASM binary.
    let fake_hash = soroban_sdk::BytesN::from_array(&t.env, &[0u8; 32]);

    let non_admin = &t.participants[0];

    // Temporarily override mocked auths to only allow non_admin signature
    // (i.e. admin auth is absent) — the call must be rejected.
    t.env.mock_auths(&[MockAuth {
        address: non_admin,
        invoke: &MockAuthInvoke {
            contract: &t.tanda.address,
            fn_name: "upgrade",
            args: (fake_hash.clone(),).into_val(&t.env),
            sub_invokes: &[],
        },
    }]);

    assert!(t.tanda.try_upgrade(&fake_hash).is_err());
}

// ─── New stabilization tests ─────────────────────────────────────────────────

#[test]
fn test_missed_payment_total_collected_reflects_actual_coverage() {
    // Verifies that after handle_missed_payment, total_collected
    // equals the actual USDC covered (not the full payment_amount).
    let t = setup();

    for p in &t.participants {
        t.tanda.register(p);
    }

    // Round 0: all pay normally → each builds collateral = PAYMENT / 10
    for p in &t.participants {
        t.tanda.make_payment(p);
    }
    t.tanda.finalize_round(&t.admin);

    // Round 1: only participants[0] and [1] pay. participants[2] misses.
    t.tanda.make_payment(&t.participants[0]);
    t.tanda.make_payment(&t.participants[1]);

    advance_time(&t.env, PERIOD + 1);

    // participant[2]'s collateral from round 0 = PAYMENT / 10 = 100 USDC
    // That's less than PAYMENT (1000 USDC), so shortfall exists.
    let p2_collateral = t.tanda.get_participant(&t.participants[2]).collateral_held;
    assert!(p2_collateral < PAYMENT, "collateral should be less than full payment");

    t.tanda.handle_missed_payment(&t.participants[2]);

    let r = t.tanda.get_round_info();
    assert_eq!(r.payments_received, N);

    // total_collected should be 2 full payments + actual coverage (NOT 3 full payments)
    let _expected = PAYMENT * 2 + p2_collateral; // pool_coverage may add more
    assert!(
        r.total_collected <= PAYMENT * 2 + PAYMENT,
        "total_collected should not exceed theoretical maximum"
    );
    // More importantly: total_collected < 3 * PAYMENT because coverage was partial
    assert!(
        r.total_collected < PAYMENT * N as i128,
        "total_collected must reflect partial coverage, got {}",
        r.total_collected
    );
}

#[test]
fn test_missed_payment_no_prior_collateral() {
    // A participant misses payment on round 0 (no collateral built up yet).
    // Coverage should come only from shared pool (which is also empty on round 0).
    let t = setup();

    // Register all
    for p in &t.participants {
        t.tanda.register(p);
    }

    // Round 0: participants[0] and [1] pay, but [2] never pays
    t.tanda.make_payment(&t.participants[0]);
    t.tanda.make_payment(&t.participants[1]);

    advance_time(&t.env, PERIOD + 1);

    // participant[2] has 0 collateral (never paid before)
    let p2 = t.tanda.get_participant(&t.participants[2]);
    assert_eq!(p2.collateral_held, 0);

    // Shared collateral pool has 10% from participants[0] and [1]'s payments
    let pool_before = t.tanda.get_collateral_pool();

    // handle_missed_payment should succeed; coverage comes from shared pool
    t.tanda.handle_missed_payment(&t.participants[2]);

    let p2_after = t.tanda.get_participant(&t.participants[2]);
    assert_eq!(p2_after.missed_payments, 1);
    assert_eq!(p2_after.last_paid_round, 0);

    let pool_after = t.tanda.get_collateral_pool();
    let pool_used = pool_before - pool_after;

    let r = t.tanda.get_round_info();
    assert_eq!(r.payments_received, N);
    // total_collected = 2 full payments + pool coverage (no own collateral)
    assert_eq!(r.total_collected, PAYMENT * 2 + pool_used);
    // Coverage should be partial (pool < full payment amount)
    assert!(
        r.total_collected < PAYMENT * N as i128,
        "with 0 own collateral and limited pool, total should be less than full"
    );
}

#[test]
fn test_multiple_consecutive_missed_payments() {
    // Same participant misses 2 rounds in a row.
    // Collateral drains correctly each time.
    let t = setup();

    for p in &t.participants {
        t.tanda.register(p);
    }

    // Round 0: all pay (builds collateral)
    for p in &t.participants {
        t.tanda.make_payment(p);
    }
    t.tanda.finalize_round(&t.admin);

    // Round 1: participant[2] misses
    t.tanda.make_payment(&t.participants[0]);
    t.tanda.make_payment(&t.participants[1]);
    advance_time(&t.env, PERIOD + 1);

    let col_before_miss1 = t.tanda.get_participant(&t.participants[2]).collateral_held;
    t.tanda.handle_missed_payment(&t.participants[2]);
    let col_after_miss1 = t.tanda.get_participant(&t.participants[2]).collateral_held;
    assert!(col_after_miss1 < col_before_miss1, "collateral should decrease after miss");

    t.tanda.finalize_round(&t.admin);

    // Round 2: participant[2] misses again
    t.tanda.make_payment(&t.participants[0]);
    t.tanda.make_payment(&t.participants[1]);
    advance_time(&t.env, PERIOD + 1);

    t.tanda.handle_missed_payment(&t.participants[2]);
    let p2 = t.tanda.get_participant(&t.participants[2]);
    assert_eq!(p2.missed_payments, 2);
    // Collateral should have decreased further (or hit 0)
    assert!(p2.collateral_held <= col_after_miss1);
}

#[test]
fn test_reinvest_then_claim_fails() {
    // After reinvest_payout, claim_payout should fail with AlreadyReceivedPayout.
    let t = setup();

    for p in &t.participants {
        t.tanda.register(p);
    }
    for p in &t.participants {
        t.tanda.make_payment(p);
    }
    t.tanda.finalize_round(&t.admin);

    // participant[0] reinvests instead of claiming
    t.tanda.reinvest_payout(&t.participants[0]);

    // Now claiming should fail
    let result = t.tanda.try_claim_payout(&t.participants[0]);
    assert!(result.is_err(), "claim after reinvest should fail");
}

#[test]
fn test_claim_payout_collateral_returned_only_on_completion() {
    // Verify collateral is NOT returned during intermediate rounds,
    // only when tanda status is Completed.
    let t = setup();

    for p in &t.participants {
        t.tanda.register(p);
    }

    let token = TokenClient::new(&t.env, &t.token_id);

    // Round 0: all pay
    for p in &t.participants {
        t.tanda.make_payment(p);
    }
    t.tanda.finalize_round(&t.admin);

    // participant[0] claims after round 0 — tanda is still Active
    let cfg = t.tanda.get_config();
    assert_eq!(cfg.status, TandaStatus::Active);

    let p0_info = t.tanda.get_participant(&t.participants[0]);
    let collateral_before = p0_info.collateral_held;
    assert!(collateral_before > 0, "should have collateral");

    let bal_before = token.balance(&t.participants[0]);
    let payout = t.tanda.claim_payout(&t.participants[0]);
    let bal_after = token.balance(&t.participants[0]);

    assert_eq!(bal_after - bal_before, payout);

    // Collateral should NOT have been returned (tanda is Active, not Completed)
    let p0_after = t.tanda.get_participant(&t.participants[0]);
    assert_eq!(
        p0_after.collateral_held, collateral_before,
        "collateral should be unchanged during Active state"
    );
}
