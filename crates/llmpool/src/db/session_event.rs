use crate::db::DbPool;
use crate::models::{BalanceChange, NewBalanceChange, NewSessionEvent, SessionEvent};

/// Create a new session event entry and return the inserted SessionEvent
#[allow(dead_code)]
pub async fn create_session_event(
    pool: &DbPool,
    new_event: &NewSessionEvent,
) -> Result<SessionEvent, sqlx::Error> {
    sqlx::query_as::<_, SessionEvent>(
        "INSERT INTO session_events (session_id, session_index, account_id, model_id, api_key_id, input_token_price, input_tokens, output_token_price, output_tokens, event_data)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         ON CONFLICT (session_id, session_index) DO UPDATE SET event_data = EXCLUDED.event_data, input_token_price = EXCLUDED.input_token_price, input_tokens = EXCLUDED.input_tokens, output_token_price = EXCLUDED.output_token_price, output_tokens = EXCLUDED.output_tokens
         RETURNING *",
    )
    .bind(&new_event.session_id)
    .bind(new_event.session_index)
    .bind(new_event.account_id)
    .bind(new_event.model_id)
    .bind(new_event.api_key_id)
    .bind(&new_event.input_token_price)
    .bind(new_event.input_tokens)
    .bind(&new_event.output_token_price)
    .bind(new_event.output_tokens)
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
        "INSERT INTO session_events (session_id, session_index, account_id, model_id, api_key_id, input_token_price, input_tokens, output_token_price, output_tokens, event_data)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         ON CONFLICT (session_id, session_index) DO UPDATE SET event_data = EXCLUDED.event_data, input_token_price = EXCLUDED.input_token_price, input_tokens = EXCLUDED.input_tokens, output_token_price = EXCLUDED.output_token_price, output_tokens = EXCLUDED.output_tokens
         RETURNING *",
    )
    .bind(&new_event.session_id)
    .bind(new_event.session_index)
    .bind(new_event.account_id)
    .bind(new_event.model_id)
    .bind(new_event.api_key_id)
    .bind(&new_event.input_token_price)
    .bind(new_event.input_tokens)
    .bind(&new_event.output_token_price)
    .bind(new_event.output_tokens)
    .bind(&new_event.event_data)
    .fetch_one(&mut **tx)
    .await
}

/// Get a single session event by its ID
pub async fn get_session_event_by_id(
    pool: &DbPool,
    event_id: i64,
) -> Result<Option<SessionEvent>, sqlx::Error> {
    sqlx::query_as::<_, SessionEvent>("SELECT * FROM session_events WHERE id = $1")
        .bind(event_id)
        .fetch_optional(pool)
        .await
}

/// List session events with optional session_id filter, using cursor-based pagination.
/// `start` is the event ID to start from (exclusive, i.e. returns events with id > start).
/// If `start` is None or 0, starts from the beginning.
/// Returns up to `count + 1` rows so the caller can determine if there are more results.
pub async fn list_session_events_cursor(
    pool: &DbPool,
    session_id: Option<&str>,
    start: i64,
    count: i64,
) -> Result<Vec<SessionEvent>, sqlx::Error> {
    if let Some(sid) = session_id {
        sqlx::query_as::<_, SessionEvent>(
            "SELECT * FROM session_events WHERE session_id = $1 AND id > $2 ORDER BY id ASC LIMIT $3",
        )
        .bind(sid)
        .bind(start)
        .bind(count + 1)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, SessionEvent>(
            "SELECT * FROM session_events WHERE id > $1 ORDER BY id ASC LIMIT $2",
        )
        .bind(start)
        .bind(count + 1)
        .fetch_all(pool)
        .await
    }
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
    sqlx::query_as::<_, BalanceChange>("SELECT * FROM balance_changes WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
}

/// Create a new balance change entry
#[allow(dead_code)]
pub async fn create_balance_change(
    pool: &DbPool,
    new_change: &NewBalanceChange,
) -> Result<BalanceChange, sqlx::Error> {
    sqlx::query_as::<_, BalanceChange>(
        "INSERT INTO balance_changes (account_id, unique_request_id, content) VALUES ($1, $2, $3) RETURNING *",
    )
    .bind(new_change.account_id)
    .bind(&new_change.unique_request_id)
    .bind(&new_change.content)
    .fetch_one(pool)
    .await
}

/// Create a new balance change entry using an existing transaction, returning the created record
pub async fn create_balance_change_with_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    new_change: &NewBalanceChange,
) -> Result<BalanceChange, sqlx::Error> {
    sqlx::query_as::<_, BalanceChange>(
        "INSERT INTO balance_changes (account_id, unique_request_id, content) VALUES ($1, $2, $3) RETURNING *",
    )
    .bind(new_change.account_id)
    .bind(&new_change.unique_request_id)
    .bind(&new_change.content)
    .fetch_one(&mut **tx)
    .await
}
