# SCIMD

See example directory for usage.

Currently only used for testing, ask Simon or Janne for more information.

## Usage

A SCIM resource type is a struct implementing `Resource` (what it is called, where it lives, what
its schema looks like) plus a `ResourceService<R>` implementation (what to actually do). Register
one per resource type; scimd derives the routes, `/ResourceTypes` and `/Schemas` from what you
registered.

```rust
let server = scimd::AppBuilder::new()
    .register::<scimd::models::User, _>(UserService::new())
    .register::<Device, _>(DeviceService::new())  // a custom resource type; no framework changes
    .bearer_token(token)
    .build();
server.run().await?;
```

Every method of `ResourceService` defaults to returning `MethodNotImplemented`, so implement only
the operations your product supports - the rest answer `501` on their own. `User` and `Group` ship
with scimd as the two resource types RFC 7643 predefines; see `example/` for a custom one.

### Schema extensions

A resource type can carry extra attributes namespaced under their own URN - `User` ships with the
Enterprise User extension. An extension is a field with a `serde(rename)` of the URN, plus a
declaration:

```rust
fn extensions() -> Vec<Extension> {
    vec![Extension::optional(Lease::schema())]
}
```

scimd advertises it under `schemaExtensions` in `/ResourceTypes`, serves its definition from
`/Schemas`, and adds the URN to a response's `schemas` array whenever the extension is actually
populated - so a service cannot forget to declare data it returned.

### Search and bulk

`POST {endpoint}/.search` takes the same query a `GET` list takes, but in the body, for filters too
long or too sensitive for a URL. `POST /v2/.search` at the root asks every registered resource type
at once and concatenates the answers; a resource type whose service does not implement `list` just
contributes nothing. Paging a root search is approximate, since `startIndex` and `count` reach each
backend individually - page a single resource type's `.search` when it has to be exact.

`POST /v2/Bulk` applies a batch of create/replace/patch/delete operations, dispatching each one to
the same service method the individual routes use. It is off by default:

```rust
.with_bulk(true, Some(50), Some(1024 * 1024))  // supported, maxOperations, maxPayloadSize
```

A failing operation is reported with its own status and error document and does not abort the
batch, unless the request sets `failOnErrors`. A `POST` may carry a `bulkId`, which later
operations in the same batch reference as `bulkId:<value>` in a path or anywhere in their data;
resolution is sequential, so the operation defining a `bulkId` must come before the ones using it.

`startIndex`/`count` on a list request are passed to the service untouched - scimd never slices a
result set itself. A service that pages should answer with `ListResponse::paginated`.

### Metrics

`GET /metrics` serves Prometheus text exposition. It is off by default:

```rust
.with_metrics(true)
```

The endpoint sits outside the SCIM scope, next to `/health`, so a scraper reaches it without a
bearer token and without asking for `application/scim+json` - **keeping it off a public interface
is the operator's job**. Pass your own registry instead, and a service's own metrics are served
from the same endpoint:

```rust
let registry = prometheus::Registry::new();  // scimd::MetricsRegistry
.metrics_registry(registry.clone())          // implies with_metrics(true)
```

| Metric | Labels |
|---|---|
| `scimd_http_requests_total` | `endpoint`, `method`, `status` |
| `scimd_http_requests_duration_seconds` | `endpoint`, `method`, `status` |
| `scimd_scim_operations_total` | `resource_type`, `operation`, `source`, `outcome` |
| `scimd_scim_operation_duration_seconds` | `resource_type`, `operation` |
| `scimd_bulk_operations_per_request` | - |
| `scimd_registered_resource_types` | - |

The HTTP metrics alone would not tell you much: `POST /Bulk` is one request that performs any
number of operations, and a root `POST /.search` fans out across every resource type and skips the
ones answering `501`. So each resource operation is counted separately, labelled with the resource
type, one of `list`/`create`/`get`/`replace`/`patch`/`delete`/`search`, whether it arrived on its
own route or inside a batch (`source`), and how it ended - `success`, or the error it raised
(`not_found`, `not_implemented`, `conflict`, ...). A batch operation rejected before it named a
resource type, such as one with an unparseable path, is reported in the `/Bulk` response and
counted in the batch size, but has no resource type to be counted under.

Every label draws from a closed set, so nothing a client sends can create a time series: `endpoint`
is the matched route pattern (`/v2/Users/{id}`, never a concrete id) and paths matching no route at
all collapse into `UNKNOWN`. `/health` is excluded.

## Directory structure

```
example/ # bin example: the built-in User resource, plus a custom Device resource
gitlabd/ # microservice that creates users in gitlab on request
scimd/   # library service that implements the interface and provides all necessary http endpoints
```

## Native dependencies for nix package in helsinki

Add the following e.g. to the corresponding Cargo.toml

```
[package.metadata.nix]
nativeBuildInputs = [
    ["pkg-config"],
]
buildInputs = [
    ["openssl"],
]
```
