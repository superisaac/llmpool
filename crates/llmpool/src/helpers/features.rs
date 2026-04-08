use crate::db::DbPool;

/// Detect and save features for an upstream by calling both the OpenAI and Anthropic
/// feature detection routines.
///
/// - The OpenAI routine probes each model for chat-completion, embedding, image-generation,
///   speech, and responses-API support.
/// - The Anthropic routine probes the upstream for `/v1/messages` support and updates
///   `has_messages` for all models belonging to that upstream.
pub async fn detect_and_save_features(
    pool: &DbPool,
    name: &str,
    api_key: &str,
    api_base: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // OpenAI feature detection (chat, embedding, image, speech, responses API)
    crate::openai::features::detect_and_save_features(pool, name, api_key, api_base).await?;

    // Anthropic feature detection (has_messages / /v1/messages support)
    crate::anthropic::features::detect_and_save_features(pool, api_key, api_base).await?;

    Ok(())
}

/// Detect and update features for a single model (by its database primary key) by calling  
/// both the OpenAI and Anthropic feature detection routines.
///
/// Returns the final `LLMModel` after both updates have been applied.
pub async fn detect_and_update_model_features(
    pool: &DbPool,
    model_pk: i32,
) -> Result<crate::models::LLMModel, Box<dyn std::error::Error + Send + Sync>> {
    // OpenAI feature detection (chat, embedding, image, speech)
    crate::openai::features::detect_and_update_model_features(pool, model_pk).await?;

    // Anthropic feature detection (has_messages)
    let updated_model =
        crate::anthropic::features::detect_and_update_model_features(pool, model_pk).await?;

    Ok(updated_model)
}
