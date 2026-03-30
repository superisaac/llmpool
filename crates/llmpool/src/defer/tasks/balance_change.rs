use apalis::prelude::*;
use tracing::{info, warn};

use crate::db::{self, DbPool};
use crate::defer::BalanceChangeTask;
use crate::models::BalanceChangeContent;

/// Handle a balance change entry from the async task queue.
///
/// This task runs within a single database transaction:
/// 1. Fetches the balance change record by ID (with FOR UPDATE lock)
/// 2. Checks if it has already been applied (skips if so)
/// 3. Parses its content JSON into a BalanceChangeContent enum
/// 4. Applies the balance change to the user's balance
/// 5. Marks the balance change as applied
pub async fn settle_balance_change(entry: BalanceChangeTask, pool: Data<DbPool>) {
    let balance_change_id = entry.balance_change_id;

    info!(
        balance_change_id = balance_change_id,
        "Processing deferred balance change"
    );

    // Begin a database transaction for the entire operation
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(e) => {
            warn!(
                error = %e,
                balance_change_id = balance_change_id,
                "Failed to begin database transaction"
            );
            return;
        }
    };

    // 1. Fetch the balance change record (with FOR UPDATE lock)
    let balance_change = match db::session_event::find_balance_change_by_id_with_tx(
        &mut tx,
        balance_change_id,
    )
    .await
    {
        Ok(Some(bc)) => bc,
        Ok(None) => {
            warn!(
                balance_change_id = balance_change_id,
                "Balance change record not found"
            );
            return;
        }
        Err(e) => {
            warn!(
                error = %e,
                balance_change_id = balance_change_id,
                "Failed to fetch balance change record"
            );
            return;
        }
    };

    // 2. Check if already applied
    if balance_change.is_applied {
        info!(
            balance_change_id = balance_change_id,
            "Balance change already applied, skipping"
        );
        return;
    }

    // 3. Parse the content JSON into BalanceChangeContent
    let content: BalanceChangeContent = match serde_json::from_value(balance_change.content) {
        Ok(content) => content,
        Err(e) => {
            warn!(
                error = %e,
                balance_change_id = balance_change_id,
                "Failed to parse balance change content"
            );
            return;
        }
    };

    // 4. Apply the balance change to the consumer's balance within the same transaction
    let updated_balance =
        match db::fund::apply_balance_change_with_tx(&mut tx, balance_change.account_id, &content)
            .await
        {
            Ok(ub) => ub,
            Err(e) => {
                warn!(
                    error = %e,
                    balance_change_id = balance_change_id,
                    account_id = balance_change.account_id,
                    "Failed to apply balance change"
                );
                return;
            }
        };

    // 5. Mark the balance change as applied
    if let Err(e) =
        db::session_event::mark_balance_change_applied_with_tx(&mut tx, balance_change.id).await
    {
        warn!(
            error = %e,
            balance_change_id = balance_change_id,
            "Failed to mark balance change as applied"
        );
        return;
    }

    // Commit the transaction
    match tx.commit().await {
        Ok(()) => {
            info!(
                balance_change_id = balance_change_id,
                account_id = balance_change.account_id,
                cash = %updated_balance.cash,
                credit = %updated_balance.credit,
                debt = %updated_balance.debt,
                "Successfully applied balance change"
            );
        }
        Err(e) => {
            warn!(
                error = %e,
                balance_change_id = balance_change_id,
                account_id = balance_change.account_id,
                "Failed to commit balance change transaction"
            );
        }
    }
}
