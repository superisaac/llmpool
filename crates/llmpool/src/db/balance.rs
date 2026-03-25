use bigdecimal::BigDecimal;
use chrono::Utc;

use crate::db::DbPool;
use crate::models::{BalanceChangeContent, NewUserBalance, UpdateUserBalance, UserBalance};

/// Find a user's balance by user_id
pub async fn find_user_balance(
    pool: &DbPool,
    user_id: i32,
) -> Result<Option<UserBalance>, sqlx::Error> {
    sqlx::query_as::<_, UserBalance>("SELECT * FROM user_balances WHERE user_id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
}

/// Apply a balance change to a user's balance.
///
/// - SpendToken: balance -= (input_spend_amount + output_spend_amount)
/// - Deposit: if debt > 0, pay off debt first; any remaining amount is added to balance
/// - Withdraw: balance -= amount
///
/// If balance goes negative after a spend or withdraw, the deficit is added to debt
/// and balance is set to zero.
///
/// If the user has no existing balance record, one is created automatically.
#[allow(dead_code)]
pub async fn apply_balance_change(
    pool: &DbPool,
    user_id: i32,
    content: &BalanceChangeContent,
) -> Result<UserBalance, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let result = apply_balance_change_with_tx(&mut tx, user_id, content).await?;
    tx.commit().await?;
    Ok(result)
}

/// Apply a balance change to a user's balance using an existing transaction.
///
/// This variant is useful when you want to run the operation within an existing transaction.
pub async fn apply_balance_change_with_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: i32,
    content: &BalanceChangeContent,
) -> Result<UserBalance, sqlx::Error> {
    // Find or create the user balance
    let user_balance: Option<UserBalance> =
        sqlx::query_as::<_, UserBalance>("SELECT * FROM user_balances WHERE user_id = $1")
            .bind(user_id)
            .fetch_optional(&mut **tx)
            .await?;

    let user_balance = match user_balance {
        Some(ub) => ub,
        None => {
            // Create a new balance record with zero balance, zero credit, and zero debt
            let new_balance = NewUserBalance {
                user_id,
                cash: BigDecimal::from(0),
                credit: BigDecimal::from(0),
                debt: BigDecimal::from(0),
            };
            sqlx::query_as::<_, UserBalance>(
                "INSERT INTO user_balances (user_id, cash, credit, debt)
                 VALUES ($1, $2, $3, $4)
                 RETURNING *",
            )
            .bind(new_balance.user_id)
            .bind(&new_balance.cash)
            .bind(&new_balance.credit)
            .bind(&new_balance.debt)
            .fetch_one(&mut **tx)
            .await?
        }
    };

    let zero = BigDecimal::from(0);

    let (new_cash, new_credit, new_debt) = match content {
        BalanceChangeContent::SpendToken(spend) => {
            let spend_amount = &spend.input_spend_amount + &spend.output_spend_amount;
            // Deduct from credit first
            let remaining_after_credit = &user_balance.credit - &spend_amount;
            if remaining_after_credit >= zero {
                // Credit alone covers the spend
                (
                    user_balance.cash.clone(),
                    remaining_after_credit,
                    user_balance.debt.clone(),
                )
            } else {
                // Credit is exhausted, deduct remainder from cash
                let remainder = zero.clone() - &remaining_after_credit;
                let remaining_after_cash = &user_balance.cash - &remainder;
                if remaining_after_cash >= zero {
                    // Cash covers the remainder
                    (
                        remaining_after_cash,
                        zero.clone(),
                        user_balance.debt.clone(),
                    )
                } else {
                    // Cash is also exhausted, add deficit to debt
                    let deficit = zero.clone() - &remaining_after_cash;
                    (zero.clone(), zero.clone(), &user_balance.debt + &deficit)
                }
            }
        }
        BalanceChangeContent::Deposit { amount } => {
            // Deposit does not change credit
            if user_balance.debt > zero {
                // Pay off debt first, then add remaining to cash
                let remaining_debt = &user_balance.debt - amount;
                if remaining_debt > zero {
                    // Deposit is not enough to cover all debt
                    (
                        user_balance.cash.clone(),
                        user_balance.credit.clone(),
                        remaining_debt,
                    )
                } else {
                    // Deposit covers all debt, remainder goes to cash
                    let surplus = zero.clone() - &remaining_debt;
                    (
                        &user_balance.cash + &surplus,
                        user_balance.credit.clone(),
                        zero,
                    )
                }
            } else {
                let new_bal = &user_balance.cash + amount;
                (
                    new_bal,
                    user_balance.credit.clone(),
                    user_balance.debt.clone(),
                )
            }
        }
        BalanceChangeContent::Withdraw { amount } => {
            // Withdraw does not change credit
            let remaining = &user_balance.cash - amount;
            if remaining < zero {
                let deficit = zero.clone() - &remaining;
                (
                    zero,
                    user_balance.credit.clone(),
                    &user_balance.debt + &deficit,
                )
            } else {
                (
                    remaining,
                    user_balance.credit.clone(),
                    user_balance.debt.clone(),
                )
            }
        }
    };

    let update = UpdateUserBalance {
        cash: Some(new_cash),
        credit: Some(new_credit),
        debt: Some(new_debt),
        updated_at: Some(Utc::now().naive_utc()),
    };

    sqlx::query_as::<_, UserBalance>(
        "UPDATE user_balances SET cash = $1, credit = $2, debt = $3, updated_at = $4
         WHERE id = $5
         RETURNING *",
    )
    .bind(&update.cash)
    .bind(&update.credit)
    .bind(&update.debt)
    .bind(&update.updated_at)
    .bind(user_balance.id)
    .fetch_one(&mut **tx)
    .await
}
