/// On-chain event types for the Tanda contract.
///
/// Using the `#[contractevent]` macro (soroban-sdk v25) so that off-chain
/// indexers (wallets, explorers) can decode events from typed schemas.
use soroban_sdk::{contractevent, Address};

// ── Event types ────────────────────────────────────────────────────────────

/// Participant successfully registered.
#[contractevent(topics = ["registered"])]
pub struct RegisteredEvent {
    pub participant: Address,
    pub turn: u32,
}

/// Tanda moved to Active status.
#[contractevent(topics = ["tanda_start"])]
pub struct TandaStartedEvent {
    pub start_time: u64,
    pub max_participants: u32,
}

/// A payment was successfully processed.
/// `collateral` = USDC retained (10%), `invested` = USDC forwarded to Etherfuse.
#[contractevent(topics = ["paid"])]
pub struct PaymentMadeEvent {
    pub participant: Address,
    pub round: u32,
    pub amount: i128,
    pub collateral: i128,
    pub invested: i128,
    pub cetes_minted: i128,
}

/// A participant missed their payment; collateral was used to cover.
#[contractevent(topics = ["missed"])]
pub struct PaymentMissedEvent {
    pub participant: Address,
    pub round: u32,
    pub own_collateral_used: i128,
    pub pool_used: i128,
}

/// A round was finalised and the next round started (or tanda completed).
#[contractevent(topics = ["round_end"])]
pub struct RoundFinalizedEvent {
    pub round: u32,
    pub beneficiary: Address,
}

/// Beneficiary withdrew their payout from the tanda.
#[contractevent(topics = ["claimed"])]
pub struct PayoutClaimedEvent {
    pub participant: Address,
    pub payout_round: u32,
    pub principal: i128,
    pub yield_amount: i128,
    pub collateral_returned: i128,
}

/// Beneficiary elected to leave their payout invested in CETES.
#[contractevent(topics = ["reinvested"])]
pub struct PayoutReinvestedEvent {
    pub participant: Address,
    pub payout_round: u32,
    pub cetes_kept: i128,
}
