//! Prometheus instrumentation, exposed at `GET /metrics`.
//!
//! Two layers are recorded, because neither can be derived from the other:
//!
//! - **HTTP**, by `actix-web-prom`: count and duration per route, method and status. The `endpoint`
//!   label is the *matched route pattern* - `/v2/Users/{id}`, never the concrete path.
//! - **SCIM**, by this module: one observation per resource operation. `POST /Bulk` is a single
//!   HTTP request that performs any number of operations, and a root `POST /.search` fans out
//!   across every registered resource type and skips the ones that answer `MethodNotImplemented`;
//!   in both cases the HTTP layer sees one request and cannot report what actually happened
//!   underneath it.
//!
//! Every label here draws from a closed set - the registered resource types, the seven operations,
//! the two sources, the error variants.

use std::time::Instant;

use actix_web_prom::{PrometheusMetrics, PrometheusMetricsBuilder};
use prometheus::{Histogram, HistogramOpts, HistogramVec, IntCounterVec, IntGauge, Opts, Registry};

use crate::Result;

const NAMESPACE: &str = "scimd";

pub(crate) const ENDPOINT: &str = "/metrics";

/// What the `endpoint` label becomes for a request that matched no route. Without this every
/// bot-probed path on the internet would be its own series.
const UNMATCHED: &str = "UNKNOWN";

/// `/Bulk` batch sizes. Bounded above by `maxOperations`, which RFC 7644 lets a provider set
/// anywhere, so the top bucket is generous.
const BULK_SIZE_BUCKETS: &[f64] = &[1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0];

/// Which of a resource endpoint's operations was invoked.
#[derive(Clone, Copy)]
pub(crate) enum Operation {
    List,
    Create,
    Get,
    Replace,
    Patch,
    Delete,
    Search,
}

impl Operation {
    fn as_str(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Create => "create",
            Self::Get => "get",
            Self::Replace => "replace",
            Self::Patch => "patch",
            Self::Delete => "delete",
            Self::Search => "search",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Source {
    Http,
    Bulk,
}

impl Source {
    fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Bulk => "bulk",
        }
    }
}

const SUCCESS: &str = "success";

pub(crate) struct Metrics {
    prom: PrometheusMetrics,
    operations: IntCounterVec,
    duration: HistogramVec,
    bulk_size: Histogram,
    resource_types: IntGauge,
}

impl Metrics {
    pub(crate) fn new(registry: Option<Registry>) -> anyhow::Result<Self> {
        let mut builder = PrometheusMetricsBuilder::new(NAMESPACE)
            .endpoint(ENDPOINT)
            .exclude("/health")
            .mask_unmatched_patterns(UNMATCHED);
        if let Some(registry) = registry {
            builder = builder.registry(registry);
        }
        let prom = builder
            .build()
            .map_err(|e| anyhow::anyhow!("could not set up the HTTP metrics: {e}"))?;

        let operations = IntCounterVec::new(
            Opts::new(
                "scim_operations_total",
                "Total number of SCIM resource operations, counted per resource type and outcome",
            )
            .namespace(NAMESPACE),
            &["resource_type", "operation", "source", "outcome"],
        )?;
        let duration = HistogramVec::new(
            HistogramOpts::new(
                "scim_operation_duration_seconds",
                "Time a SCIM resource operation spent in the service backing it",
            )
            .namespace(NAMESPACE),
            &["resource_type", "operation"],
        )?;
        let bulk_size = Histogram::with_opts(
            HistogramOpts::new(
                "bulk_operations_per_request",
                "Number of operations carried by a single POST /Bulk request",
            )
            .namespace(NAMESPACE)
            .buckets(BULK_SIZE_BUCKETS.to_vec()),
        )?;
        let resource_types = IntGauge::with_opts(
            Opts::new(
                "registered_resource_types",
                "Number of resource types registered on this server",
            )
            .namespace(NAMESPACE),
        )?;

        prom.registry.register(Box::new(operations.clone()))?;
        prom.registry.register(Box::new(duration.clone()))?;
        prom.registry.register(Box::new(bulk_size.clone()))?;
        prom.registry.register(Box::new(resource_types.clone()))?;

        Ok(Self {
            prom,
            operations,
            duration,
            bulk_size,
            resource_types,
        })
    }

    pub(crate) fn middleware(&self) -> PrometheusMetrics {
        self.prom.clone()
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "only the tests read metrics back")
    )]
    pub(crate) fn registry(&self) -> &Registry {
        &self.prom.registry
    }

    pub(crate) fn set_resource_types(&self, count: usize) {
        self.resource_types.set(count as i64);
    }

    pub(crate) fn observe_bulk_size(&self, operations: usize) {
        self.bulk_size.observe(operations as f64);
    }

    fn observe_operation(
        &self,
        resource_type: &str,
        operation: Operation,
        source: Source,
        outcome: &str,
        elapsed: f64,
    ) {
        self.operations
            .with_label_values(&[resource_type, operation.as_str(), source.as_str(), outcome])
            .inc();
        self.duration
            .with_label_values(&[resource_type, operation.as_str()])
            .observe(elapsed);
    }
}

/// Times `call` and records what it answered.
pub(crate) async fn observe<T>(
    metrics: Option<&Metrics>,
    resource_type: &str,
    operation: Operation,
    source: Source,
    call: impl Future<Output = Result<T>>,
) -> Result<T> {
    let Some(metrics) = metrics else {
        return call.await;
    };

    let started = Instant::now();
    let result = call.await;
    let outcome = match &result {
        Ok(_) => SUCCESS,
        Err(e) => e.kind(),
    };
    metrics.observe_operation(
        resource_type,
        operation,
        source,
        outcome,
        started.elapsed().as_secs_f64(),
    );
    result
}
