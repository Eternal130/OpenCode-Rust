//! OTel telemetry skeleton (RW-06) — port of `packages/util/src/observability.ts`
//! + `observability/otlp.ts` (v2.0.24), compiled only behind the
//! non-default `otel` cargo feature. Feature OFF (release default) keeps
//! the dependency closure byte-identical: no tracing/otel crates, no
//! subscriber, no middleware — zero behavioral change.
//!
//! REF semantics mirrored:
//! - **Toggle**: telemetry activates only when `OTEL_EXPORTER_OTLP_ENDPOINT`
//!   is set (REF `Otlp.tracingLayer`: `if (!options?.endpoint) return
//!   Layer.empty`). No endpoint → complete no-op.
//! - **Resource** (REF `Otlp.resource`): `service.name = "opencode"`,
//!   `service.version`, `deployment.environment.name` (channel),
//!   `opencode.client`, `opencode.run` / `service.instance.id` (one run id
//!   per process), plus `OTEL_RESOURCE_ATTRIBUTES` (`k=v,k=v`,
//!   percent-decoded, ANY malformed entry discards the whole list).
//! - **Exporter**: OTLP over gRPC at
//!   `${OTEL_EXPORTER_OTLP_ENDPOINT}/v1/traces` (REF appends `/v1/traces`)
//!   with headers parsed from `OTEL_EXPORTER_OTLP_HEADERS`
//!   (comma-separated `k=v`, REF `parseHeaders`; `k=a=b` keeps `a=b`).
//! - **Spans**: every HTTP request gets one span (REF's Effect HTTP server
//!   spans); the skeleton names them `http.request` with method / path /
//!   status-code attributes.
//! - **Failure fallback**: exporter init failure logs a warning and keeps
//!   running uninstrumented (REF `Layer.catchCause(() => local)`).

use axum::middleware::Next;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::{WithExportConfig, WithTonicConfig};
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use tracing_subscriber::prelude::*;

/// REF `runID()` — one id per process (shared by `opencode.run` and
/// `service.instance.id`).
fn run_id() -> String {
    use std::sync::OnceLock;
    static RUN: OnceLock<String> = OnceLock::new();
    RUN.get_or_init(|| uuid::Uuid::new_v4().to_string()).clone()
}

/// REF `parseHeaders`: `k=v,k2=v2` → map; `k=a=b` keeps `a=b` as the
/// value; unset/empty → none.
fn parse_headers(value: Option<&str>) -> Option<Vec<(String, String)>> {
    let value = value?;
    if value.is_empty() {
        return None;
    }
    Some(
        value
            .split(',')
            .filter_map(|entry| {
                entry.split_once('=').map(|(key, rest)| (key.to_owned(), rest.to_owned()))
            })
            .collect(),
    )
}

fn header_metadata(value: Option<&str>) -> Option<tonic::metadata::MetadataMap> {
    let pairs = parse_headers(value)?;
    let mut map = tonic::metadata::MetadataMap::new();
    for (key, value) in pairs {
        if let Ok(metadata_value) = tonic::metadata::MetadataValue::try_from(value.as_str()) {
            if let Ok(name) = tonic::metadata::MetadataKey::from_bytes(key.as_bytes()) {
                map.insert(name, metadata_value);
            }
        }
    }
    Some(map)
}

/// REF `resourceAttributes()`: `k=v,k2=v2` with percent-decoding; ANY
/// malformed entry discards the whole list (`{}`).
fn resource_attributes_env() -> Vec<(String, String)> {
    let Some(value) = std::env::var("OTEL_RESOURCE_ATTRIBUTES").ok().filter(|v| !v.is_empty())
    else {
        return Vec::new();
    };
    value
        .split(',')
        .map(|entry| {
            let index = entry.find('=').ok_or(())?;
            if index < 1 {
                return Err(());
            }
            Ok((percent_decode(&entry[..index])?, percent_decode(&entry[index + 1..])?))
        })
        .collect::<Result<Vec<_>, ()>>()
        .unwrap_or_default()
}

fn percent_decode(input: &str) -> Result<String, ()> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = input.get(index + 1..index + 3).ok_or(())?;
            let value = u8::from_str_radix(hex, 16).map_err(|_| ())?;
            out.push(value);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).map_err(|_| ())
}

/// REF `resource(app)`: serviceName "opencode", the app attributes, plus
/// `OTEL_RESOURCE_ATTRIBUTES`.
fn resource(version: &str, channel: &str) -> Resource {
    let mut attributes: Vec<KeyValue> = vec![
        KeyValue::new("deployment.environment.name", channel.to_owned()),
        KeyValue::new("opencode.client", "opencode"),
        KeyValue::new("opencode.run", run_id()),
        KeyValue::new("service.instance.id", run_id()),
        KeyValue::new("service.version", version.to_owned()),
    ];
    attributes.extend(
        resource_attributes_env().into_iter().map(|(key, value)| KeyValue::new(key, value)),
    );
    Resource::builder().with_service_name("opencode").with_attributes(attributes).build()
}

/// Initializes the global tracing subscriber with the OTLP span exporter.
///
/// - No `OTEL_EXPORTER_OTLP_ENDPOINT` → returns `None` without touching
///   the process (REF `Layer.empty`).
/// - Channel: `OPENCODE_CHANNEL`, `"local"` default like REF's app
///   default.
///
/// The returned provider keeps the batch exporter's background drain task
/// alive for the process lifetime (REF closes the OTel scope on server
/// shutdown; the skeleton relies on drop-at-exit draining).
#[must_use]
pub fn init(version: &str) -> Option<SdkTracerProvider> {
    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok().filter(|v| !v.is_empty())?;
    let channel = std::env::var("OPENCODE_CHANNEL").unwrap_or_else(|_| "local".to_owned());
    let url = format!("{endpoint}/v1/traces");

    let mut builder = opentelemetry_otlp::SpanExporter::builder().with_tonic().with_endpoint(url);
    if let Some(metadata) =
        header_metadata(std::env::var("OTEL_EXPORTER_OTLP_HEADERS").ok().as_deref())
    {
        builder = builder.with_metadata(metadata);
    }
    let exporter = match builder.build() {
        Ok(exporter) => exporter,
        Err(error) => {
            // REF: observability layer failure falls back to local
            // logging (`Layer.catchCause(() => local)`).
            eprintln!("[observability] OTLP exporter init failed: {error:?}");
            return None;
        }
    };
    let provider = SdkTracerProvider::builder()
        .with_resource(resource(version, &channel))
        .with_batch_exporter(exporter)
        .build();
    let tracer = provider.tracer("opencode");
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    tracing_subscriber::registry().with(otel_layer).init();
    Some(provider)
}

/// Per-request span middleware (`http.request`), mounted only with the
/// `otel` feature. Without an initialized subscriber the span macros are
/// no-ops, so behavior is unchanged even in `otel` builds without an
/// endpoint.
pub async fn request_span(request: axum::extract::Request, next: Next) -> axum::response::Response {
    let method = request.method().as_str().to_owned();
    let path = request.uri().path().to_owned();
    let span = tracing::info_span!(
        "http.request",
        "http.request.method" = %method,
        "url.path" = %path,
        "http.response.status_code" = tracing::field::Empty,
    );
    use tracing::Instrument as _;
    async move {
        let response = next.run(request).await;
        tracing::Span::current()
            .record("http.response.status_code", response.status().as_u16() as i64);
        response
    }
    .instrument(span)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_headers_mirrors_ref() {
        assert_eq!(parse_headers(None), None);
        assert_eq!(parse_headers(Some("")), None);
        assert_eq!(
            parse_headers(Some("api-key=abc,other=a=b")),
            Some(vec![
                ("api-key".to_owned(), "abc".to_owned()),
                ("other".to_owned(), "a=b".to_owned())
            ])
        );
    }

    #[test]
    fn resource_attributes_discards_malformed_lists() {
        // Valid list round-trips (percent-decoded).
        assert_eq!(
            resource_attributes_env_for("k=v,k2=percent%20decoded"),
            vec![("k".to_owned(), "v".to_owned()), ("k2".to_owned(), "percent decoded".to_owned())]
        );
        // Any malformed entry (no '=' / bad escape) discards the WHOLE list.
        assert!(resource_attributes_env_for("novalue").is_empty());
        assert!(resource_attributes_env_for("k=bad%2").is_empty());
    }

    fn resource_attributes_env_for(value: &str) -> Vec<(String, String)> {
        std::env::set_var("OTEL_RESOURCE_ATTRIBUTES", value);
        let parsed = resource_attributes_env();
        std::env::remove_var("OTEL_RESOURCE_ATTRIBUTES");
        parsed
    }
}
