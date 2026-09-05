//! The HTTP layer's Prometheus instrumentation, and the `GET /metrics` exposition: count and
//! duration per route, method and status, where the `endpoint` label is the *matched route
//! pattern* - `/v2/Users/{id}`, never the concrete path. The SCIM layer's own is
//! `scimd::ScimMetrics`.

use actix_web_prom::{PrometheusMetrics, PrometheusMetricsBuilder};
use scimd::MetricsRegistry as Registry;

const NAMESPACE: &str = "scimd";

pub(crate) const ENDPOINT: &str = "/metrics";

/// What the `endpoint` label becomes for a request that matched no route. Without this every
/// bot-probed path on the internet would be its own series.
const UNMATCHED: &str = "UNKNOWN";

/// The middleware that both records the HTTP series and serves [`ENDPOINT`].
pub(crate) fn http_middleware(registry: Registry) -> anyhow::Result<PrometheusMetrics> {
    PrometheusMetricsBuilder::new(NAMESPACE)
        .endpoint(ENDPOINT)
        .exclude("/health")
        .mask_unmatched_patterns(UNMATCHED)
        .registry(registry)
        .build()
        .map_err(|e| anyhow::anyhow!("could not set up the HTTP metrics: {e}"))
}
