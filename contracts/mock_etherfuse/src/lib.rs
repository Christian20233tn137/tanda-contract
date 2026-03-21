//! Mock Etherfuse CETES stablebond contract for testnet.
//!
//! Simulates the Etherfuse protocol:
//!   deposit(depositor, usdc_amount) → cetes_tokens  (1:1, USDC already received)
//!   redeem(recipient, cetes_amount) → usdc_amount   (transfers USDC back)
//!   get_nav()                       → i128          (always 1_000_000 = 1.0)
//!   balance(address)                → i128
//!
//! On mainnet replace this with the official Etherfuse stablebond contract address.

#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Env,
    token::Client as TokenClient,
};

#[contracttype]
enum DataKey {
    Token,
}

#[contract]
pub struct MockEtherfuse;

#[contractimpl]
impl MockEtherfuse {
    /// Store the USDC token contract address at deployment time.
    pub fn __constructor(env: Env, token: Address) {
        env.storage().instance().set(&DataKey::Token, &token);
    }

    /// Accept USDC (already transferred to this contract) and return CETES tokens 1:1.
    pub fn deposit(_env: Env, _depositor: Address, usdc_amount: i128) -> i128 {
        usdc_amount
    }

    /// Burn CETES tokens and transfer USDC back to recipient (1:1).
    pub fn redeem(env: Env, recipient: Address, cetes_amount: i128) -> i128 {
        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let token = TokenClient::new(&env, &token_addr);
        token.transfer(&env.current_contract_address(), &recipient, &cetes_amount);
        cetes_amount
    }

    /// Net Asset Value = 1_000_000 (1.0 scaled to 6 dp). No yield in mock.
    pub fn get_nav(_env: Env) -> i128 {
        1_000_000
    }

    /// CETES balance (not tracked in mock — returns 0).
    pub fn balance(_env: Env, _address: Address) -> i128 {
        0
    }
}
