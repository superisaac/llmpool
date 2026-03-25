use opentelemetry::global;
use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::{EnvFilter, Registry, layer::SubscriberExt, util::SubscriberInitExt};

/// Initialize OpenTelemetry tracing with OTLP exporter.
///
/// This sets up:
/// - An OTLP gRPC exporter (default endpoint: http://localhost:4317, configurable via OTEL_EXPORTER_OTLP_ENDPOINT)
/// - A tracing-subscriber with both a fmt layer (for console output) and an OpenTelemetry layer
/// - Environment filter via RUST_LOG (defaults to "info")
///
/// Returns the TracerProvider so it can be shut down gracefully.
pub fn init_telemetry() -> SdkTracerProvider {
    // Build the OTLP exporter (gRPC/tonic by default)
    let otlp_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .build()
        .expect("Failed to create OTLP span exporter");

    // Build the tracer provider with batch export
    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(otlp_exporter)
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_service_name("llmpool")
                .build(),
        )
        .build();

    // Set as global provider
    global::set_tracer_provider(tracer_provider.clone());

    // Get a tracer for the OpenTelemetry layer
    let tracer = tracer_provider.tracer("llmpool");

    // Build the OpenTelemetry tracing layer
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    // Build env filter (defaults to "info")
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // Build the fmt layer for console output
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(false)
        .with_level(true);

    // Compose and install the subscriber
    Registry::default()
        .with(env_filter)
        .with(fmt_layer)
        .with(otel_layer)
        .init();

    tracer_provider
}

/// Gracefully shut down the tracer provider, flushing any pending spans.
pub fn shutdown_telemetry(provider: SdkTracerProvider) {
    if let Err(e) = provider.shutdown() {
        eprintln!("Error shutting down tracer provider: {:?}", e);
    }
}
