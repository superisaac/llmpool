use bigdecimal::BigDecimal;
use chrono::Utc;

use crate::db::DbPool;
use crate::models::{BalanceChangeContent, Fund, NewFund, UpdateFund};

/// Find a user's fund by user_id
pub async fn find_user_fund(
    pool: &DbPool,
    user_id: i32,
) -> Result<Option<Fund>, sqlx::Error> {
    sqlx::query_as::<_, Fund>("SELECT * FROM funds WHERE user_id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
}

/// Apply a balance change to a user's fund.
///
/// - SpendToken: fund -= (input_spend_amount + output_spend_amount)
/// - Deposit: if debt > 0, pay off debt first; any remaining amount is added to cash
/// - Withdraw: cash -= amount
///
/// If cash goes negative after a spend or withdraw, the deficit is added to debt
/// and cash is set to zero.
///
/// If the user has no existing fund record, one is created automatically.
#[allow(dead_code)]
pub async fn apply_balance_change(
    pool: &DbPool,
    user_id: i32,
    content: &BalanceChangeContent,
) -> Result<Fund, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let result = apply_balance_change_with_tx(&mut tx, user_id, content).await?;
    tx.commit().await?;
    Ok(result)
}

/// Apply a balance change to a user's fund using an existing transaction.
///
/// This variant is useful when you want to run the operation within an existing transaction.
pub async fn apply_balance_change_with_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: i32,
    content: &BalanceChangeContent,
) -> Result<Fund, sqlx::Error> {
    // Find or create the user fund
    let user_fund: Option<Fund> =
        sqlx::query_as::<_, Fund>("SELECT * FROM funds WHERE user_id = $1")
            .bind(user_id)
            .fetch_optional(&mut **tx)
            .await?;

    let user_fund = match user_fund {
        Some(uf) => uf,
        None => {
            // Create a new fund record with zero cash, zero credit, and zero debt
            let new_fund = NewFund {
                user_id,
                cash: BigDecimal::from(0),
                credit: BigDecimal::from(0),
                debt: BigDecimal::from(0),
            };
            sqlx::query_as::<_, Fund>(
                "INSERT INTO funds (user_id, cash, credit, debt)
                 VALUES ($1, $2, $3, $4)
                 RETURNING *",
            )
            .bind(new_fund.user_id)
            .bind(&new_fund.cash)
            .bind(&new_fund.credit)
            .bind(&new_fund.debt)
            .fetch_one(&mut **tx)
            .await?
        }
    };

    let zero = BigDecimal::from(0);

    let (new_cash, new_credit, new_debt) = match content {
        BalanceChangeContent::SpendToken(spend) => {
            let spend_amount = &spend.input_spend_amount + &spend.output_spend_amount;
            // Deduct from credit first
            let remaining_after_credit = &user_fund.credit - &spend_amount;
            if remaining_after_credit >= zero {
                // Credit alone covers the spend
                (
                    user_fund.cash.clone(),
                    remaining_after_credit,
                    user_fund.debt.clone(),
                )
            } else {
                // Credit is exhausted, deduct remainder from cash
                let remainder = zero.clone() - &remaining_after_credit;
                let remaining_after_cash = &user_fund.cash - &remainder;
                if remaining_after_cash >= zero {
                    // Cash covers the remainder
                    (
                        remaining_after_cash,
                        zero.clone(),
                        user_fund.debt.clone(),
                    )
                } else {
                    // Cash is also exhausted, add deficit to debt
                    let deficit = zero.clone() - &remaining_after_cash;
                    (zero.clone(), zero.clone(), &user_fund.debt + &deficit)
                }
            }
        }
        BalanceChangeContent::Deposit { amount } => {
            // Deposit does not change credit
            if user_fund.debt > zero {
                // Pay off debt first, then add remaining to cash
                let remaining_debt = &user_fund.debt - amount;
                if remaining_debt > zero {
                    // Deposit is not enough to cover all debt
                    (
                        user_fund.cash.clone(),
                        user_fund.credit.clone(),
                        remaining_debt,
                    )
                } else {
                    // Deposit covers all debt, remainder goes to cash
                    let surplus = zero.clone() - &remaining_debt;
                    (
                        &user_fund.cash + &surplus,
                        user_fund.credit.clone(),
                        zero,
                    )
                }
            } else {
                let new_bal = &user_fund.cash + amount;
                (
                    new_bal,
                    user_fund.credit.clone(),
                    user_fund.debt.clone(),
                )
            }
        }
        BalanceChangeContent::Withdraw { amount } => {
            // Withdraw does not change credit
            let remaining = &user_fund.cash - amount;
            if remaining < zero {
                let deficit = zero.clone() - &remaining;
                (
                    zero,
                    user_fund.credit.clone(),
                    &user_fund.debt + &deficit,
                )
            } else {
                (
                    remaining,
                    user_fund.credit.clone(),
                    user_fund.debt.clone(),
                )
            }
        }
        BalanceChangeContent::Credit { amount } => {
            // Credit: if debt > 0, pay off debt first; any remaining amount is added to credit
            if user_fund.debt > zero {
                let remaining_debt = &user_fund.debt - amount;
                if remaining_debt > zero {
                    // Credit is not enough to cover all debt
                    (
                        user_fund.cash.clone(),
                        user_fund.credit.clone(),
                        remaining_debt,
                    )
                } else {
                    // Credit covers all debt, remainder goes to credit field
                    let surplus = zero.clone() - &remaining_debt;
                    (
                        user_fund.cash.clone(),
                        &user_fund.credit + &surplus,
                        zero,
                    )
                }
            } else {
                let new_credit = &user_fund.credit + amount;
                (
                    user_fund.cash.clone(),
                    new_credit,
                    user_fund.debt.clone(),
                )
            }
        }
    };

    let update = UpdateFund {
        cash: Some(new_cash),
        credit: Some(new_credit),
        debt: Some(new_debt),
        updated_at: Some(Utc::now().naive_utc()),
    };

    sqlx::query_as::<_, Fund>(
        "UPDATE funds SET cash = $1, credit = $2, debt = $3, updated_at = $4
         WHERE id = $5
         RETURNING *",
    )
    .bind(&update.cash)
    .bind(&update.credit)
    .bind(&update.debt)
    .bind(&update.updated_at)
    .bind(user_fund.id)
    .fetch_one(&mut **tx)
    .await
}
