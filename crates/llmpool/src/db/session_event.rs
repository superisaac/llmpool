use crate::db::DbPool;
use crate::models::{BalanceChange, NewBalanceChange, NewSessionEvent, SessionEvent};

/// Create a new session event entry and return the inserted SessionEvent
#[allow(dead_code)]
pub async fn create_session_event(
    pool: &DbPool,
    new_event: &NewSessionEvent,
) -> Result<SessionEvent, sqlx::Error> {
    sqlx::query_as::<_, SessionEvent>(
        "INSERT INTO session_events (session_id, user_id, model_id, event_data)
         VALUES ($1, $2, $3, $4)
         RETURNING *",
    )
    .bind(&new_event.session_id)
    .bind(new_event.user_id)
    .bind(new_event.model_id)
    .bind(&new_event.event_data)
    .fetch_one(pool)
    .await
}

/// Create a new session event entry using an existing transaction
pub async fn create_session_event_with_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    new_event: &NewSessionEvent,
) -> Result<SessionEvent, sqlx::Error> {
    sqlx::query_as::<_, SessionEvent>(
        "INSERT INTO session_events (session_id, user_id, model_id, event_data)
         VALUES ($1, $2, $3, $4)
         RETURNING *",
    )
    .bind(&new_event.session_id)
    .bind(new_event.user_id)
    .bind(new_event.model_id)
    .bind(&new_event.event_data)
    .fetch_one(&mut **tx)
    .await
}

/// Find a balance change by its ID
#[allow(dead_code)]
pub async fn find_balance_change_by_id(
    pool: &DbPool,
    id: i64,
) -> Result<Option<BalanceChange>, sqlx::Error> {
    sqlx::query_as::<_, BalanceChange>("SELECT * FROM balance_changes WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Find a balance change by its ID using an existing transaction (with FOR UPDATE lock)
pub async fn find_balance_change_by_id_with_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: i64,
) -> Result<Option<BalanceChange>, sqlx::Error> {
    sqlx::query_as::<_, BalanceChange>(
        "SELECT * FROM balance_changes WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
}

/// Mark a balance change as applied using an existing transaction
pub async fn mark_balance_change_applied_with_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE balance_changes SET is_applied = TRUE WHERE id = $1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Create a new balance change entry
#[allow(dead_code)]
pub async fn create_balance_change(
    pool: &DbPool,
    new_change: &NewBalanceChange,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO balance_changes (user_id, content) VALUES ($1, $2)")
        .bind(new_change.user_id)
        .bind(&new_change.content)
        .execute(pool)
        .await?;
    Ok(())
}

/// Create a new balance change entry using an existing transaction, returning the created record
pub async fn create_balance_change_with_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    new_change: &NewBalanceChange,
) -> Result<BalanceChange, sqlx::Error> {
    sqlx::query_as::<_, BalanceChange>(
        "INSERT INTO balance_changes (user_id, content) VALUES ($1, $2) RETURNING *",
    )
    .bind(new_change.user_id)
    .bind(&new_change.content)
    .fetch_one(&mut **tx)
    .await
}
