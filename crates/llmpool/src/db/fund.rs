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
/// - SpendToken: fund.balance -= (input_spend_amount + output_spend_amount)
/// - Deposit / AddCredit: fund.balance += amount
/// - Withdraw: fund.balance -= amount
///
/// balance can be negative (indicating debt).
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
            // Create a new fund record with zero balance
            let new_fund = NewFund {
                account_id,
                balance: BigDecimal::from(0),
            };
            sqlx::query_as::<_, Fund>(
                "INSERT INTO funds (account_id, balance)
                 VALUES ($1, $2)
                 RETURNING *",
            )
            .bind(new_fund.account_id)
            .bind(&new_fund.balance)
            .fetch_one(&mut **tx)
            .await?
        }
    };

    // Compute new balance; balance can be negative (debt)
    let new_balance = match content {
        BalanceChangeContent::SpendToken(spend) => {
            let spend_amount = &spend.input_spend_amount + &spend.output_spend_amount;
            &account_fund.balance - &spend_amount
        }
        BalanceChangeContent::Deposit { amount } | BalanceChangeContent::AddCredit { amount } => {
            &account_fund.balance + amount
        }
        BalanceChangeContent::Withdraw { amount } => &account_fund.balance - amount,
    };

    let update = UpdateFund {
        balance: Some(new_balance),
        updated_at: Some(Utc::now().naive_utc()),
    };

    sqlx::query_as::<_, Fund>(
        "UPDATE funds SET balance = $1, updated_at = $2
         WHERE id = $3
         RETURNING *",
    )
    .bind(&update.balance)
    .bind(&update.updated_at)
    .bind(account_fund.id)
    .fetch_one(&mut **tx)
    .await
}
