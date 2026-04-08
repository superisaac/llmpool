use crate::db::DbPool;

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
