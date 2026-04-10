use bigdecimal::BigDecimal;
use chrono::Utc;

use crate::db::DbPool;
use crate::db::subscription::{
    get_current_subscription_with_tx, increment_subscription_usage_with_tx,
};
use crate::models::{BalanceChange, BalanceChangeContent, NewWallet, UpdateWallet, Wallet};

/// Find a account's wallet by account_id
pub async fn find_account_wallet(
    pool: &DbPool,
    account_id: i32,
) -> Result<Option<Wallet>, sqlx::Error> {
    sqlx::query_as::<_, Wallet>("SELECT * FROM wallets WHERE account_id = $1")
        .bind(account_id)
        .fetch_optional(pool)
        .await
}

/// Mark a balance change as applied using an existing transaction
pub async fn mark_balance_change_applied_with_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: i32,
    subscription_id: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE balance_changes SET is_applied = TRUE, subscription_id = $2 WHERE id = $1")
        .bind(id)
        .bind(subscription_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Apply a balance change to a user's wallet.
///
/// - SpendToken: wallet.balance -= (input_spend_amount + output_spend_amount)
/// - Deposit / AddCredit: wallet.balance += amount
/// - Withdraw: wallet.balance -= amount
///
/// balance can be negative (indicating debt).
///
/// If the account has no existing wallet record, one is created automatically.
#[allow(dead_code)]
pub async fn apply_balance_change(
    pool: &DbPool,
    balance_change: &BalanceChange,
    content: &BalanceChangeContent,
) -> Result<Wallet, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let result = apply_balance_change_with_tx(&mut tx, balance_change, content).await?;
    tx.commit().await?;
    Ok(result)
}

/// Apply a balance change to a user's wallet using an existing transaction.
///
/// This variant is useful when you want to run the operation within an existing transaction.
pub async fn apply_balance_change_with_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    balance_change: &BalanceChange,
    // account_id: i32,
    content: &BalanceChangeContent,
) -> Result<Wallet, sqlx::Error> {
    let account_id = balance_change.account_id;
    // Find or create the account wallet
    let account_wallet: Option<Wallet> =
        sqlx::query_as::<_, Wallet>("SELECT * FROM wallets WHERE account_id = $1")
            .bind(account_id)
            .fetch_optional(&mut **tx)
            .await?;

    let account_wallet = match account_wallet {
        Some(uf) => uf,
        None => {
            // Create a new wallet record with zero balance
            let new_wallet = NewWallet {
                account_id,
                balance: BigDecimal::from(0),
            };
            sqlx::query_as::<_, Wallet>(
                "INSERT INTO wallets (account_id, balance)
                 VALUES ($1, $2)
                 RETURNING *",
            )
            .bind(new_wallet.account_id)
            .bind(&new_wallet.balance)
            .fetch_one(&mut **tx)
            .await?
        }
    };

    let mut subscription_id: i32 = 0;
    // Compute new balance; balance can be negative (debt)
    let new_balance = match content {
        BalanceChangeContent::SpendToken(spend) => {
            // If SpendToken, try to find and update an active subscription first
            let total_tokens = spend.input_tokens + spend.output_tokens;
            let spend_amount = spend.input_spend_amount.clone() + spend.output_spend_amount.clone();
            if let Some(subscription) =
                get_current_subscription_with_tx(tx, account_id, total_tokens).await?
            {
                subscription_id = subscription.id;
                increment_subscription_usage_with_tx(tx, &subscription, total_tokens, spend_amount)
                    .await?;
                account_wallet.balance.clone()
            } else {
                account_wallet.balance.clone() - spend_amount
            }
        }
        BalanceChangeContent::Deposit { amount } | BalanceChangeContent::AddCredit { amount } => {
            &account_wallet.balance + amount
        }
        BalanceChangeContent::Withdraw { amount } => &account_wallet.balance - amount,
    };

    let return_wallet = if new_balance != account_wallet.balance {
        let update = UpdateWallet {
            balance: Some(new_balance),
            updated_at: Some(Utc::now().naive_utc()),
        };

        sqlx::query_as::<_, Wallet>(
            "UPDATE wallets SET balance = $1, updated_at = $2
            WHERE id = $3
            RETURNING *",
        )
        .bind(&update.balance)
        .bind(&update.updated_at)
        .bind(account_wallet.id)
        .fetch_one(&mut **tx)
        .await?
    } else {
        account_wallet
    };
    mark_balance_change_applied_with_tx(tx, balance_change.id, subscription_id).await?;
    Ok(return_wallet)
}
