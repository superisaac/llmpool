use crate::db::DbPool;
use crate::models::{Subscription, SubscriptionPlan};
use bigdecimal::BigDecimal;

// ============================================================
// SubscriptionPlan DB operations
// ============================================================

/// Create a new subscription plan
pub async fn create_subscription_plan(
    pool: &DbPool,
    description: &str,
    total_token_limit: i64,
    time_span: i64,
    money_limit: &bigdecimal::BigDecimal,
    sort_order: i64,
) -> Result<SubscriptionPlan, sqlx::Error> {
    sqlx::query_as::<_, SubscriptionPlan>(
        r#"
        INSERT INTO subscription_plans
            (description, total_token_limit, time_span, money_limit, sort_order)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING *
        "#,
    )
    .bind(description)
    .bind(total_token_limit)
    .bind(time_span)
    .bind(money_limit)
    .bind(sort_order)
    .fetch_one(pool)
    .await
}

/// Get a subscription plan by ID
pub async fn get_subscription_plan_by_id(
    pool: &DbPool,
    plan_id: i64,
) -> Result<Option<SubscriptionPlan>, sqlx::Error> {
    sqlx::query_as::<_, SubscriptionPlan>("SELECT * FROM subscription_plans WHERE id = $1")
        .bind(plan_id)
        .fetch_optional(pool)
        .await
}

/// Count total number of subscription plans
pub async fn count_subscription_plans(pool: &DbPool) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM subscription_plans")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// List subscription plans with pagination
pub async fn list_subscription_plans_paginated(
    pool: &DbPool,
    offset: i64,
    limit: i64,
) -> Result<Vec<SubscriptionPlan>, sqlx::Error> {
    sqlx::query_as::<_, SubscriptionPlan>(
        "SELECT * FROM subscription_plans ORDER BY sort_order DESC, id ASC LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

/// Update a subscription plan by ID
pub async fn update_subscription_plan(
    pool: &DbPool,
    plan_id: i64,
    description: Option<&str>,
    total_token_limit: Option<i64>,
    time_span: Option<i64>,
    money_limit: Option<&bigdecimal::BigDecimal>,
    sort_order: Option<i64>,
    status: Option<&str>,
) -> Result<SubscriptionPlan, sqlx::Error> {
    sqlx::query_as::<_, SubscriptionPlan>(
        r#"
        UPDATE subscription_plans SET
            description = COALESCE($2, description),
            total_token_limit = COALESCE($3, total_token_limit),
            time_span = COALESCE($4, time_span),
            money_limit = COALESCE($5, money_limit),
            sort_order = COALESCE($6, sort_order),
            status = COALESCE($7, status),
            updated_at = NOW()
        WHERE id = $1
        RETURNING *
        "#,
    )
    .bind(plan_id)
    .bind(description)
    .bind(total_token_limit)
    .bind(time_span)
    .bind(money_limit)
    .bind(sort_order)
    .bind(status)
    .fetch_one(pool)
    .await
}

/// Deactivate (soft-delete) a subscription plan by setting status = 'deactive'
pub async fn cancel_subscription_plan(
    pool: &DbPool,
    plan_id: i64,
) -> Result<SubscriptionPlan, sqlx::Error> {
    sqlx::query_as::<_, SubscriptionPlan>(
        r#"
        UPDATE subscription_plans SET status = 'deactive', updated_at = NOW()
        WHERE id = $1
        RETURNING *
        "#,
    )
    .bind(plan_id)
    .fetch_one(pool)
    .await
}

// ============================================================
// Subscription DB operations
// ============================================================

/// Create a new subscription for an account
pub async fn create_subscription(
    pool: &DbPool,
    account_id: i64,
    plan_id: i64,
) -> Result<Subscription, sqlx::Error> {
    sqlx::query_as::<_, Subscription>(
        r#"
        INSERT INTO subscriptions (account_id, plan_id)
        VALUES ($1, $2)
        RETURNING *
        "#,
    )
    .bind(account_id)
    .bind(plan_id)
    .fetch_one(pool)
    .await
}

/// Get a subscription by ID
pub async fn get_subscription_by_id(
    pool: &DbPool,
    subscription_id: i64,
) -> Result<Option<Subscription>, sqlx::Error> {
    sqlx::query_as::<_, Subscription>("SELECT * FROM subscriptions WHERE id = $1")
        .bind(subscription_id)
        .fetch_optional(pool)
        .await
}

/// Count subscriptions with optional filters
pub async fn count_subscriptions(
    pool: &DbPool,
    account_id: Option<i64>,
    status: Option<&str>,
) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*) FROM subscriptions
        WHERE ($1::BIGINT IS NULL OR account_id = $1)
          AND ($2::VARCHAR IS NULL OR status = $2)
        "#,
    )
    .bind(account_id)
    .bind(status)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// List subscriptions with optional filters and pagination
pub async fn list_subscriptions_paginated(
    pool: &DbPool,
    account_id: Option<i64>,
    status: Option<&str>,
    offset: i64,
    limit: i64,
) -> Result<Vec<Subscription>, sqlx::Error> {
    sqlx::query_as::<_, Subscription>(
        r#"
        SELECT * FROM subscriptions
        WHERE ($1::BIGINT IS NULL OR account_id = $1)
          AND ($2::VARCHAR IS NULL OR status = $2)
        ORDER BY sort_order DESC, id DESC
        LIMIT $3 OFFSET $4
        "#,
    )
    .bind(account_id)
    .bind(status)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

/// Update a subscription's status
pub async fn update_subscription_status(
    pool: &DbPool,
    subscription_id: i64,
    status: &str,
) -> Result<Subscription, sqlx::Error> {
    sqlx::query_as::<_, Subscription>(
        r#"
        UPDATE subscriptions SET status = $2, updated_at = NOW()
        WHERE id = $1
        RETURNING *
        "#,
    )
    .bind(subscription_id)
    .bind(status)
    .fetch_one(pool)
    .await
}

/// Deactivate a subscription by setting status = 'deactive'
pub async fn cancel_subscription(
    pool: &DbPool,
    subscription_id: i64,
) -> Result<Subscription, sqlx::Error> {
    sqlx::query_as::<_, Subscription>(
        r#"
        UPDATE subscriptions SET status = 'deactive', updated_at = NOW()
        WHERE id = $1
        RETURNING *
        "#,
    )
    .bind(subscription_id)
    .fetch_one(pool)
    .await
}

/// Find the current active subscription for an account.
///
/// Looks up the subscription ordered by sort_order DESC,
/// and returns the first one that:
/// - has subscription status == "active"
/// - has already started (start_at <= NOW())
/// - has not yet expired (end_at > NOW())
/// - has enough remaining token quota (total_token_limit >= used_total_tokens + consumed_tokens)
///
/// Returns None if no such subscription exists.
pub async fn get_current_subscription(
    pool: &DbPool,
    account_id: i64,
    consumed_tokens: i64,
) -> Result<Option<Subscription>, sqlx::Error> {
    sqlx::query_as::<_, Subscription>(
        r#"
        SELECT *
        FROM subscriptions
        WHERE account_id = $1
          AND status = 'active'
          AND start_at <= NOW()
          AND end_at > NOW()
          AND total_token_limit >= used_total_tokens + $2
        ORDER BY sort_order DESC
        LIMIT 1
        "#,
    )
    .bind(account_id)
    .bind(consumed_tokens)
    .fetch_optional(pool)
    .await
}

/// Find the current active subscription for an account within an existing transaction.
///
/// Same logic as `get_current_subscription` but operates within a provided transaction.
pub async fn get_current_subscription_with_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    account_id: i64,
    consumed_tokens: i64,
) -> Result<Option<Subscription>, sqlx::Error> {
    sqlx::query_as::<_, Subscription>(
        r#"
        SELECT *
        FROM subscriptions
        WHERE account_id = $1
          AND status = 'active'
          AND start_at <= NOW()
          AND end_at > NOW()
          AND total_token_limit >= used_total_tokens + $2
        ORDER BY sort_order DESC
        LIMIT 1
        "#,
    )
    .bind(account_id)
    .bind(consumed_tokens)
    .fetch_optional(&mut **tx)
    .await
}

pub async fn increment_subscription_usage_with_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    subscription: &Subscription,
    total_tokens: i64,
    total_spend_amount: BigDecimal,
) -> Result<(), sqlx::Error> {
    let new_used_total_tokens = subscription.used_total_tokens + total_tokens;
    let new_used_money = &subscription.used_money + total_spend_amount;

    sqlx::query(
        "UPDATE subscriptions SET used_total_tokens = $1, used_money = $2, updated_at = NOW()
            WHERE id = $3",
    )
    .bind(new_used_total_tokens)
    .bind(&new_used_money)
    .bind(subscription.id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
