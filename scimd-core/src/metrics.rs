//! The SCIM layer's Prometheus instrumentation: one observation per resource operation.
//!
//! Deliberately not the HTTP layer's. `POST /Bulk` is a single HTTP request that performs any
//! number of operations, and a root `POST /.search` fans out across every registered resource type,
//! so an HTTP-level counter sees one request and cannot report what happened underneath it. An
//! adapter that also wants per-route HTTP metrics adds them itself.

use std::time::Instant;

use prometheus::{Histogram, HistogramOpts, HistogramVec, IntCounterVec, IntGauge, Opts, Registry};
use scimd_types::Result;

const NAMESPACE: &str = "scimd";

/// `/Bulk` batch sizes. Bounded above by `maxOperations`, which RFC 7644 lets a provider set
/// anywhere, so the top bucket is generous.
const BULK_SIZE_BUCKETS: &[f64] = &[1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0];

const SUCCESS: &str = "success";

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

pub struct ScimMetrics {
    operations: IntCounterVec,
    duration: HistogramVec,
    bulk_size: Histogram,
    resource_types: IntGauge,
}

impl ScimMetrics {
    /// The registry is the caller's: a host serves it from wherever it already serves Prometheus.
    /// One registry can therefore back only one [`ScimMetrics`].
    ///
    /// # Errors
    ///
    /// If a collector cannot be built, or is already registered in `registry`.
    pub fn new(registry: &Registry) -> anyhow::Result<Self> {
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

        registry.register(Box::new(operations.clone()))?;
        registry.register(Box::new(duration.clone()))?;
        registry.register(Box::new(bulk_size.clone()))?;
        registry.register(Box::new(resource_types.clone()))?;

        Ok(Self {
            operations,
            duration,
            bulk_size,
            resource_types,
        })
    }

    pub fn set_resource_types(&self, count: usize) {
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

pub(crate) async fn observe<T>(
    metrics: Option<&ScimMetrics>,
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
