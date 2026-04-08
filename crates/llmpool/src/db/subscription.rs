use chrono::Utc;

use crate::db::DbPool;
use crate::models::{Subscription, SubscriptionPlan};

// ============================================================
// SubscriptionPlan DB operations
// ============================================================

/// Create a new subscription plan
pub async fn create_subscription_plan(
    pool: &DbPool,
    description: &str,
    input_token_limit: i64,
    output_token_limit: i64,
    money_limit: &bigdecimal::BigDecimal,
    start_at: Option<chrono::NaiveDateTime>,
    end_at: Option<chrono::NaiveDateTime>,
    sort_order: i32,
) -> Result<SubscriptionPlan, sqlx::Error> {
    sqlx::query_as::<_, SubscriptionPlan>(
        r#"
        INSERT INTO subscription_plans
            (description, input_token_limit, output_token_limit, money_limit, start_at, end_at, sort_order)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING *
        "#,
    )
    .bind(description)
    .bind(input_token_limit)
    .bind(output_token_limit)
    .bind(money_limit)
    .bind(start_at)
    .bind(end_at)
    .bind(sort_order)
    .fetch_one(pool)
    .await
}

/// Get a subscription plan by ID
pub async fn get_subscription_plan_by_id(
    pool: &DbPool,
    plan_id: i32,
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
    plan_id: i32,
    description: Option<&str>,
    input_token_limit: Option<i64>,
    output_token_limit: Option<i64>,
    money_limit: Option<&bigdecimal::BigDecimal>,
    start_at: Option<Option<chrono::NaiveDateTime>>,
    end_at: Option<Option<chrono::NaiveDateTime>>,
    sort_order: Option<i32>,
    status: Option<&str>,
) -> Result<SubscriptionPlan, sqlx::Error> {
    sqlx::query_as::<_, SubscriptionPlan>(
        r#"
        UPDATE subscription_plans SET
            description = COALESCE($2, description),
            input_token_limit = COALESCE($3, input_token_limit),
            output_token_limit = COALESCE($4, output_token_limit),
            money_limit = COALESCE($5, money_limit),
            start_at = CASE WHEN $6 THEN $7 ELSE start_at END,
            end_at = CASE WHEN $8 THEN $9 ELSE end_at END,
            sort_order = COALESCE($10, sort_order),
            status = COALESCE($11, status),
            updated_at = NOW()
        WHERE id = $1
        RETURNING *
        "#,
    )
    .bind(plan_id)
    .bind(description)
    .bind(input_token_limit)
    .bind(output_token_limit)
    .bind(money_limit)
    .bind(start_at.is_some())
    .bind(start_at.flatten())
    .bind(end_at.is_some())
    .bind(end_at.flatten())
    .bind(sort_order)
    .bind(status)
    .fetch_one(pool)
    .await
}

/// Cancel (soft-delete) a subscription plan by setting status = 'canceled'
pub async fn cancel_subscription_plan(
    pool: &DbPool,
    plan_id: i32,
) -> Result<SubscriptionPlan, sqlx::Error> {
    sqlx::query_as::<_, SubscriptionPlan>(
        r#"
        UPDATE subscription_plans SET status = 'canceled', updated_at = NOW()
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
    account_id: i32,
    plan_id: i32,
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
    subscription_id: i32,
) -> Result<Option<Subscription>, sqlx::Error> {
    sqlx::query_as::<_, Subscription>("SELECT * FROM subscriptions WHERE id = $1")
        .bind(subscription_id)
        .fetch_optional(pool)
        .await
}

/// Count subscriptions with optional filters
pub async fn count_subscriptions(
    pool: &DbPool,
    account_id: Option<i32>,
    status: Option<&str>,
) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*) FROM subscriptions
        WHERE ($1::INT IS NULL OR account_id = $1)
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
    account_id: Option<i32>,
    status: Option<&str>,
    offset: i64,
    limit: i64,
) -> Result<Vec<Subscription>, sqlx::Error> {
    sqlx::query_as::<_, Subscription>(
        r#"
        SELECT * FROM subscriptions
        WHERE ($1::INT IS NULL OR account_id = $1)
          AND ($2::VARCHAR IS NULL OR status = $2)
        ORDER BY id DESC
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
    subscription_id: i32,
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

/// Cancel a subscription by setting status = 'canceled'
pub async fn cancel_subscription(
    pool: &DbPool,
    subscription_id: i32,
) -> Result<Subscription, sqlx::Error> {
    sqlx::query_as::<_, Subscription>(
        r#"
        UPDATE subscriptions SET status = 'canceled', updated_at = NOW()
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
/// Looks up the subscription joined with its plan, ordered by plan.sort_order DESC,
/// and returns the first one that:
/// - has subscription status == "active"
/// - the plan has already started (plan.start_at IS NULL OR plan.start_at <= NOW())
/// - the plan has not yet expired (plan.end_at IS NULL OR plan.end_at > NOW())
///
/// Returns None if no such subscription exists.
pub async fn get_current_subscription(
    pool: &DbPool,
    account_id: i32,
) -> Result<Option<Subscription>, sqlx::Error> {
    let now = Utc::now().naive_utc();

    sqlx::query_as::<_, Subscription>(
        r#"
        SELECT s.*
        FROM subscriptions s
        JOIN subscription_plans p ON s.plan_id = p.id
        WHERE s.account_id = $1
          AND s.status = 'active'
          AND (p.start_at IS NULL OR p.start_at <= $2)
          AND (p.end_at IS NULL OR p.end_at > $2)
        ORDER BY p.sort_order DESC
        LIMIT 1
        "#,
    )
    .bind(account_id)
    .bind(now)
    .fetch_optional(pool)
    .await
}
