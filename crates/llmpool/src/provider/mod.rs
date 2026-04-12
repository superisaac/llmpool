pub mod types;

/// Look up a provider by its canonical name.
///
/// Returns `None` when no provider with that name is registered.
pub fn get_provider(provider_name: &str) -> Option<Box<dyn types::Provider>> {
    get_all_providers()
        .into_iter()
        .find(|p| p.provider_name() == provider_name)
}

/// Return every registered provider.
pub fn get_all_providers() -> Vec<Box<dyn types::Provider>> {
    vec![
        Box::new(crate::openai::provider::OpenAIProvider),
        Box::new(crate::anthropic::provider::AnthropicProvider),
    ]
}
