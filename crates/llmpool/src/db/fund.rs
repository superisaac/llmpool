use bigdecimal::BigDecimal;
use chrono::Utc;

use crate::db::DbPool;
use crate::models::{BalanceChangeContent, Fund, NewFund, UpdateFund};

/// Find a account's fund by account_id
pub async fn find_account_fund(
    pool: &DbPool,
    account_id: i32,
) -> Result<Option<Fund>, sqlx::Error> {
    sqlx::query_as::<_, Fund>("SELECT * FROM funds WHERE account_id = $1")
        .bind(account_id)
        .fetch_optional(pool)
        .await
}

/// Apply a balance change to a user's fund.
///
/// - SpendToken: fund.cash -= (input_spend_amount + output_spend_amount)
/// - Deposit / AddCredit: if debt > 0, pay off debt first; any remaining amount is added to cash
/// - Withdraw: cash -= amount
/// - Credit: fund.credit += amount
///
/// If cash goes negative after a spend or withdraw, the deficit is added to debt
/// and cash is set to zero.
///
/// If the account has no existing fund record, one is created automatically.
#[allow(dead_code)]
pub async fn apply_balance_change(
    pool: &DbPool,
    account_id: i32,
    content: &BalanceChangeContent,
) -> Result<Fund, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let result = apply_balance_change_with_tx(&mut tx, account_id, content).await?;
    tx.commit().await?;
    Ok(result)
}

/// Apply a balance change to a user's fund using an existing transaction.
///
/// This variant is useful when you want to run the operation within an existing transaction.
pub async fn apply_balance_change_with_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    account_id: i32,
    content: &BalanceChangeContent,
) -> Result<Fund, sqlx::Error> {
    // Find or create the account fund
    let account_fund: Option<Fund> =
        sqlx::query_as::<_, Fund>("SELECT * FROM funds WHERE account_id = $1")
            .bind(account_id)
            .fetch_optional(&mut **tx)
            .await?;

    let account_fund = match account_fund {
        Some(uf) => uf,
        None => {
            // Create a new fund record with zero cash, credit and debt
            let new_fund = NewFund {
                account_id,
                cash: BigDecimal::from(0),
                credit: BigDecimal::from(0),
                debt: BigDecimal::from(0),
            };
            sqlx::query_as::<_, Fund>(
                "INSERT INTO funds (account_id, cash, credit, debt)
                 VALUES ($1, $2, $3, $4)
                 RETURNING *",
            )
            .bind(new_fund.account_id)
            .bind(&new_fund.cash)
            .bind(&new_fund.credit)
            .bind(&new_fund.debt)
            .fetch_one(&mut **tx)
            .await?
        }
    };

    let zero = BigDecimal::from(0);

    // // Handle Credit variant separately since it updates the credit field
    // if let BalanceChangeContent::Credit { amount } = content {
    //     let new_credit = &account_fund.credit + amount;
    //     return sqlx::query_as::<_, Fund>(
    //         "UPDATE funds SET credit = $1, updated_at = $2
    //          WHERE id = $3
    //          RETURNING *",
    //     )
    //     .bind(&new_credit)
    //     .bind(Utc::now().naive_utc())
    //     .bind(account_fund.id)
    //     .fetch_one(&mut **tx)
    //     .await;
    // }

    let (new_cash, new_debt) = match content {
        BalanceChangeContent::SpendToken(spend) => {
            let spend_amount = &spend.input_spend_amount + &spend.output_spend_amount;
            let remaining = &account_fund.cash - &spend_amount;
            if remaining >= zero {
                (remaining, account_fund.debt.clone())
            } else {
                // Cash exhausted, add deficit to debt
                let deficit = zero.clone() - &remaining;
                (zero.clone(), &account_fund.debt + &deficit)
            }
        }
        BalanceChangeContent::Deposit { amount } | BalanceChangeContent::AddCredit { amount } => {
            // Both Deposit and AddCredit add to cash (pay off debt first)
            if account_fund.debt > zero {
                // pay off debt
                let remaining_debt = &account_fund.debt - amount;
                if remaining_debt > zero {
                    // Deposit is not enough to cover all debt
                    (account_fund.cash.clone(), remaining_debt)
                } else {
                    // Deposit covers all debt, remainder goes to cash
                    let surplus = zero.clone() - &remaining_debt;
                    (&account_fund.cash + &surplus, zero)
                }
            } else {
                let new_cash = &account_fund.cash + amount;
                (new_cash, account_fund.debt.clone())
            }
        }
        BalanceChangeContent::Withdraw { amount } => {
            let remaining = &account_fund.cash - amount;
            if remaining < zero {
                let deficit = zero.clone() - &remaining;
                (zero, &account_fund.debt + &deficit)
            } else {
                (remaining, account_fund.debt.clone())
            }
        }
    };

    let update = UpdateFund {
        cash: Some(new_cash),
        debt: Some(new_debt),
        updated_at: Some(Utc::now().naive_utc()),
    };

    sqlx::query_as::<_, Fund>(
        "UPDATE funds SET cash = $1, debt = $2, updated_at = $3
         WHERE id = $4
         RETURNING *",
    )
    .bind(&update.cash)
    .bind(&update.debt)
    .bind(&update.updated_at)
    .bind(account_fund.id)
    .fetch_one(&mut **tx)
    .await
}
