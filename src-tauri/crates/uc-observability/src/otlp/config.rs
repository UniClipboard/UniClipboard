pub(crate) const OTEL_ENDPOINT_VAR: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";
pub(crate) const OTEL_TRACES_ENDPOINT_VAR: &str = "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT";
pub(crate) const OTEL_HEADERS_VAR: &str = "OTEL_EXPORTER_OTLP_HEADERS";
pub(crate) const OTEL_TRACES_HEADERS_VAR: &str = "OTEL_EXPORTER_OTLP_TRACES_HEADERS";

fn env_var_is_configured(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .map(|value| !value.is_empty())
        .unwrap_or(false)
}

fn baked_value_is_configured(value: Option<&str>) -> bool {
    value.map(|raw| !raw.is_empty()).unwrap_or(false)
}

fn env_pair_is_configured(
    specific_key: &str,
    generic_key: &str,
    baked_specific: Option<&str>,
    baked_generic: Option<&str>,
) -> bool {
    env_var_is_configured(specific_key)
        || env_var_is_configured(generic_key)
        || baked_value_is_configured(baked_specific)
        || baked_value_is_configured(baked_generic)
}

pub(crate) fn prime_env_pair_from_baked(
    specific_key: &str,
    generic_key: &str,
    baked_specific: Option<&str>,
    baked_generic: Option<&str>,
) {
    if env_var_is_configured(specific_key) || env_var_is_configured(generic_key) {
        return;
    }

    if let Some(value) = baked_specific.filter(|raw| !raw.is_empty()) {
        std::env::set_var(specific_key, value);
        return;
    }

    if let Some(value) = baked_generic.filter(|raw| !raw.is_empty()) {
        std::env::set_var(generic_key, value);
    }
}

/// Returns whether OTLP trace export is explicitly configured.
///
/// Runtime env vars use the OpenTelemetry standard names directly. Compile-time
/// baked values are treated as a fallback source only so the exporter can still
/// apply its own standard endpoint resolution.
pub(crate) fn otlp_endpoint_is_configured() -> bool {
    env_pair_is_configured(
        OTEL_TRACES_ENDPOINT_VAR,
        OTEL_ENDPOINT_VAR,
        option_env!("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT"),
        option_env!("OTEL_EXPORTER_OTLP_ENDPOINT"),
    )
}

/// Backfill compile-time OTLP configuration into the standard runtime env vars.
///
/// This preserves production builds that bake configuration into the binary
/// while still letting the OpenTelemetry exporter resolve `/v1/traces` and
/// signal-specific headers by itself.
pub(crate) fn prime_runtime_otlp_env_from_baked() {
    prime_env_pair_from_baked(
        OTEL_TRACES_ENDPOINT_VAR,
        OTEL_ENDPOINT_VAR,
        option_env!("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT"),
        option_env!("OTEL_EXPORTER_OTLP_ENDPOINT"),
    );
    prime_env_pair_from_baked(
        OTEL_TRACES_HEADERS_VAR,
        OTEL_HEADERS_VAR,
        option_env!("OTEL_EXPORTER_OTLP_TRACES_HEADERS"),
        option_env!("OTEL_EXPORTER_OTLP_HEADERS"),
    );
}
