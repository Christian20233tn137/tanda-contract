use soroban_sdk::{contracttype, Address};

/// Overall lifecycle of the tanda.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TandaStatus {
    Registering, // Accepting new participants
    Active,      // Payments underway
    Completed,   // All rounds finished
}

/// Immutable configuration set at initialisation.
#[contracttype]
#[derive(Clone, Debug)]
pub struct TandaConfig {
    pub admin: Address,
    /// Total number of participants (= number of rounds).
    pub max_participants: u32,
    /// Fixed payment per period in token base units (USDC = 6 decimals).
    pub payment_amount: i128,
    /// Payment window length in seconds (e.g. 2_592_000 = 30 days).
    pub period_secs: u64,
    /// USDC contract address on Stellar.
    pub payment_token: Address,
    /// Etherfuse stablebond (CETES) contract address.
    pub cetes_token: Address,
    /// Collateral retention in basis points. Default 1_000 = 10%.
    pub collateral_bps: u32,
    pub status: TandaStatus,
    /// Unix timestamp when the tanda went Active.
    pub start_time: u64,
    /// Zero-based index of the round currently in progress.
    pub current_round: u32,
    /// == max_participants; kept for clarity.
    pub total_rounds: u32,
}

/// Per-participant mutable state.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ParticipantInfo {
    /// Zero-based payout turn (registration order).
    pub turn: u32,
    /// Cumulative USDC paid into the tanda.
    pub total_paid: i128,
    /// USDC currently held as collateral by the contract on behalf of this participant.
    pub collateral_held: i128,
    /// Round index in which the participant last made a payment.
    /// u32::MAX signals "never paid".
    pub last_paid_round: u32,
    pub has_received_payout: bool,
    pub missed_payments: u32,
}

/// Mutable state for the round currently in progress.
#[contracttype]
#[derive(Clone, Debug)]
pub struct RoundInfo {
    pub round: u32,
    /// Unix timestamp at which this round started.
    pub start_time: u64,
    /// Address that will receive the payout when this round finalises.
    pub beneficiary: Address,
    /// How many participants have paid (or been covered) this round.
    pub payments_received: u32,
    /// Gross USDC collected this round (before split).
    pub total_collected: i128,
    pub is_finalized: bool,
}

/// Aggregate CETES investment state.
#[contracttype]
#[derive(Clone, Debug)]
pub struct InvestmentPool {
    /// Total CETES tokens currently held by the contract.
    pub total_cetes_tokens: i128,
    /// Total USDC principal ever sent to Etherfuse.
    pub total_usdc_invested: i128,
    /// Cumulative yield realised on redemption.
    pub accumulated_yield: i128,
}
