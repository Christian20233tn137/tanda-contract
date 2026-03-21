use soroban_sdk::contracterror;

#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TandaError {
    // Initialisation
    AlreadyInitialized = 1,
    NotInitialized = 2,
    InvalidAmount = 3,
    // Registration
    TandaNotRegistering = 4,
    AlreadyRegistered = 5,
    TandaFull = 6,
    InsufficientBalance = 7,
    // Payment
    TandaNotActive = 8,
    RoundAlreadyFinalized = 9,
    PaymentWindowClosed = 10,
    AlreadyPaid = 11,
    PaymentWindowOpen = 12,
    // Participant lookup
    ParticipantNotFound = 13,
    // Payout
    NotYourTurn = 14,
    AlreadyReceivedPayout = 15,
    RoundNotFinalized = 16,
    NoCetesToReinvest = 17,
    // Collateral
    CollateralPoolInsufficient = 18,
    // Auth
    Unauthorized = 19,
}
