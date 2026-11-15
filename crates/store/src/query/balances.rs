//! What each account holds, confirmed or projected.

use mt_core::ids::AccountId;
use mt_core::ledger::{self, BalanceBasis};
use mt_core::money::{Currency, Money};

use crate::error::StoreError;
use crate::open::Store;
use crate::repo::{accounts, transactions};

impl Store {
    /// What one account holds.
    ///
    /// [`BalanceBasis::Confirmed`] counts only what is known to have happened;
    /// [`BalanceBasis::Projected`] counts pending rows too, which is the forecast. There is
    /// one walk behind both, which is the whole reason the pending model was adopted.
    ///
    /// # Errors
    /// [`StoreError::NotFound`] if there is no such account, [`StoreError::Rule`] if the
    /// rows will not walk — a row in another currency, or a total that overflows — or
    /// [`StoreError::Database`] if a query fails.
    pub fn account_balance(
        &self,
        account_id: AccountId,
        basis: BalanceBasis,
    ) -> Result<Money, StoreError> {
        // One lock for both reads: taken twice, a write landing between them would walk
        // this account's rows against another moment's account row.
        let connection = self.connection()?;
        let account = accounts::one(&connection, account_id)?;
        let rows = transactions::for_account(&connection, account_id)?;
        let walk = ledger::walk(&account, &rows, basis).map_err(|reason| StoreError::Rule {
            rule: "walk",
            reason: reason.to_string(),
        })?;
        Ok(walk.closing)
    }

    /// What every account holds, in one pass over the rows.
    ///
    /// One pass rather than one walk per account, so the accounts screen does not get
    /// slower in proportion to the square of how long the app has been used.
    ///
    /// # Errors
    /// As [`Store::account_balance`].
    pub fn account_balances(
        &self,
        basis: BalanceBasis,
    ) -> Result<Vec<(AccountId, Money)>, StoreError> {
        let connection = self.connection()?;
        let accounts = accounts::all(&connection)?;
        let rows = transactions::all(&connection)?;
        let balances = ledger::balances_by_account(&accounts, &rows, basis).map_err(|reason| {
            StoreError::Rule {
                rule: "balances",
                reason: reason.to_string(),
            }
        })?;
        Ok(balances.into_iter().collect())
    }

    /// The total held in each currency.
    ///
    /// Never one converted number: a total that spans currencies is an estimate that moves
    /// when the rate does, and saying so is the screen's job.
    ///
    /// # Errors
    /// As [`Store::account_balance`].
    pub fn currency_totals(
        &self,
        basis: BalanceBasis,
    ) -> Result<Vec<(Currency, Money)>, StoreError> {
        let connection = self.connection()?;
        let accounts = accounts::all(&connection)?;
        let rows = transactions::all(&connection)?;
        let totals = ledger::totals_by_currency(&accounts, &rows, basis).map_err(|reason| {
            StoreError::Rule {
                rule: "totals",
                reason: reason.to_string(),
            }
        })?;
        Ok(totals.into_iter().collect())
    }
}
