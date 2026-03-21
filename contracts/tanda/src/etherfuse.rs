/// Etherfuse Stablebond (CETES) cross-contract client.
///
/// Etherfuse tokenises Mexican government CETES bonds on Stellar.
/// Their protocol accepts USDC and returns stablebond tokens that appreciate
/// in value as the underlying CETES earns yield.
///
/// Integration flow:
///   1. Contract holds USDC after collecting participant payments.
///   2. `deposit()` — transfer invest_amount of USDC to the Etherfuse contract
///      and receive `cetes_tokens` back (minted 1-for-NAV).
///   3. `redeem()` — burn `cetes_tokens`; Etherfuse transfers USDC + yield back.
///
/// On testnet deploy the mock contract provided in `contracts/mock_etherfuse/`.
/// On mainnet use the official Etherfuse stablebond contract address.
use soroban_sdk::{Address, Env, IntoVal, Symbol, Val, Vec as SorobanVec};

pub struct EtherfuseClient<'a> {
    env: &'a Env,
    contract: Address,
}

impl<'a> EtherfuseClient<'a> {
    pub fn new(env: &'a Env, contract: &Address) -> Self {
        Self {
            env,
            contract: contract.clone(),
        }
    }

    /// Deposit `usdc_amount` USDC (already transferred to the Etherfuse contract)
    /// and receive newly minted CETES tokens.  Returns the number of tokens minted.
    ///
    /// The caller is responsible for transferring `usdc_amount` of USDC to the
    /// Etherfuse contract *before* calling this function.
    pub fn deposit(&self, depositor: &Address, usdc_amount: i128) -> i128 {
        let args: SorobanVec<Val> = (depositor.clone(), usdc_amount).into_val(self.env);
        self.env
            .invoke_contract::<i128>(&self.contract, &Symbol::new(self.env, "deposit"), args)
    }

    /// Redeem `cetes_amount` CETES tokens.
    /// Etherfuse burns the tokens and transfers USDC + accrued yield to `recipient`.
    /// Returns the total USDC received.
    pub fn redeem(&self, recipient: &Address, cetes_amount: i128) -> i128 {
        let args: SorobanVec<Val> = (recipient.clone(), cetes_amount).into_val(self.env);
        self.env
            .invoke_contract::<i128>(&self.contract, &Symbol::new(self.env, "redeem"), args)
    }
}
