# SCIMD

See example directory for usage.

Currently only used for testing, ask Simon or Janne for more information.

## Usage

A SCIM resource type is a struct implementing `Resource` (what it is called, where it lives, what
its schema looks like) plus a `ResourceService<R>` implementation (what to actually do). Register
one per resource type; scimd derives the routes, `/ResourceTypes` and `/Schemas` from what you
registered.

```rust
let scim = scimd::ScimBuilder::new()
    .register::<scimd::models::User, _>(UserService::new())
    .register::<Device, _>(DeviceService::new());  // a custom resource type; no framework changes
let app = scimd::AppBuilder::new(scim).bearer_token(token).build();
scimd_server::Server::new(app).run().await?;       // sockets, signals, /health, /metrics
```

`ScimBuilder` describes the SCIM service; `AppBuilder` adds what only the HTTP layer decides.

Every method of `ResourceService` defaults to returning `MethodNotImplemented`, so implement only
the operations your product supports - the rest answer `501` on their own. `User` and `Group` ship
with scimd as the two resource types RFC 7643 predefines; see `example/` for a custom one.

### Mounting into a server you already have

`scimd_server::Server` above binds its own sockets. A backend that already runs an actix-web
`HttpServer` does not want a second process for SCIM, so `App::configure()` hands over the routes
instead - and it needs only the `scimd` crate, not `scimd-server`:

```rust
let core = scimd::ScimBuilder::new()
    .register::<scimd::models::User, _>(UserService::new())
    .base_path("/scim/v2");
let scim = scimd::AppBuilder::new(core).bearer_token(Some(token)).build();
let scim_routes = scim.configure();           // built once, cloned per worker

HttpServer::new(move || {
    App::new()
        .service(my_existing_api())
        .configure(scim_routes.clone())
})
```

`base_path` moves everything together - the routes, `meta.location`, and the locations
`/ResourceTypes` and `/Schemas` publish - so a client that follows a link scimd gave it lands back
on the mount. A host that would rather wrap the mount in a scope of its own keeps `base_path` at
the default and says where a client actually sees it:

```rust
.location_base("/scim/v2")                    // routes stay at /v2
// ...
.service(web::scope("/scim").configure(scim_routes.clone()))
```

Authentication and `Accept` negotiation are scoped to the mount. Leaving `bearer_token` unset does
not mean "unauthenticated": a request presenting a token anyway is refused, since a token nobody
asked for says the client thinks it is talking to something else. A host whose own middleware
already checked says `.authentication_handled_upstream(true)` instead, and then scimd does not read
the header at all.

A path inside the scope that matches no route answers a SCIM error document rather than falling
through to the host's 404, and authentication runs first, so an unauthenticated request there is a
`401` before it is a `404`.

A mount does not bring `/health`, `/metrics` or `NormalizePath` - they are the host's to place, so
a trailing slash 404s unless the host wraps it.

`example-mounted/` is a working host: `cargo run -p example-mounted`.

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

To extend a built-in type, wrap it: `example/src/mail.rs` flattens `User` into a `MailUser` that
adds a mailbox extension and serves it on `/Users`.

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

`startIndex`/`count` on a list request are passed to the service untouched. A service that pages in
its backend answers with `ListResponse::paginated`; one that holds the collection whole hands it to
`scimd::filter::list_response`, below.

### PATCH

A service's `patch` gets a `PatchOp` whose operations all carry a `path`: an `add` or `replace`
without one (RFC 7644 section 3.5.2) is split into one operation per attribute before it arrives.
`Operation` reads an operation the way identity providers actually write one:

```rust
for operation in &op.operations {
    // `urn:...:User:emails[type eq "work"].value` targets "emails"
    match operation.attr().as_str() {
        "active" => set_active(operation.bool_value()),     // true, or "True"
        "emails" => set_email(operation.email_address()),   // a bare string or the emails array
        "displayname" if operation.is_remove() => clear_display_name(),
        "displayname" => set_display_name(operation.str_value()),
        _ => {}                                             // lenient: ignore what has no home
    }
}
```

`attr()` comes back lowercased, since attribute names are case-insensitive (RFC 7643 section 2.1).
`is_add()`, `is_remove()` and `is_replace()` ignore case too, because Entra ID capitalises its
operation names. Of several addresses, `email_address()` picks the one `User::primary_email` would.

### Filtering

`filter` arrives as an opaque string on `FilterQuery` and is passed to the service untouched. A
service that holds its collection whole lets scimd filter and page it:

```rust
async fn list(&self, query: FilterQuery) -> scimd::Result<ListResponse<Device>> {
    scimd::filter::list_response(self.all_devices(), &query)  // filter, then startIndex/count
}
```

Underneath, `scimd::filter::parse` turns the string into a `Filter` - the RFC 7644 section 3.4.2.2
grammar, all of it, `valuePath` brackets included - and `Filter::matches_resource` matches one
against a resource. Parsing is the service's call rather than scimd's, because a backend that
filters in its own store wants the predicate, not a decision already made for it - the AST is
public so it can be walked and pushed down instead of matched here. A filter that does not parse is
already an `Error::InvalidFilter`, so a service that writes `?` answers a `400` with `scimType:
invalidFilter` and the character the grammar broke at.

A filter comes off the network, so it has a size: `filter::MAX_LENGTH` bytes and `filter::MAX_DEPTH`
levels of `(` and `[`, both an order of magnitude past anything a client sends. Past either it is an
`Error::InvalidFilter` rather than a parse, because the depth a filter parses to is the depth
matching and dropping it recurse to, and a stack overflow in Rust takes the process with it.

Which values a filter reaches is read off the resource's JSON; the schema only decides how strings
compare. Attribute names are compared case-insensitively (RFC 7643 section 2.1), and so are string
values unless the resource type's schema declares the attribute `caseExact`: `id`, `externalId` and
`meta.resourceType` compare exactly, `userName` and `emails.value` do not. `matches_resource` and
`list_response` look this up for core attributes, sub-attributes and extension attributes alike,
while `Filter::matches` on bare JSON has no schema to ask and folds every string.

A multi-valued attribute matches when any of its values does, and a `valuePath` such as
`emails[type eq "work" and value co "@example.com"]` needs one element to satisfy every condition
rather than the attribute as a whole. An attribute the resource does not carry matches nothing, and
neither does a comparison between mismatched types - a question that cannot be asked of a resource
is answered no, never with an error. A fully qualified `attrPath` is read against the URN it names:
an extension's attributes are looked up inside that extension, a URN the resource lists among its
`schemas` reads at the root, and a URN it claims neither way matches nothing rather than falling
through to a core attribute of the same name.

A service that does not want to filter should say so twice: leave `.with_filter(...)` at its
default of `false` so `/ServiceProviderConfig` is honest, and refuse a request that carries a
`filter` anyway with `Error::InvalidFilter`.

Ignoring the parameter is the one thing not to do: it answers a narrow question with the whole
collection, and the client has no way to tell the difference.

### Metrics

Under `scimd-server`, `GET /metrics` serves Prometheus text exposition. It is off by default:

```rust
.with_metrics(true)
```

A host that mounts scimd serves its own exposition and asks `App::metrics_registry()` for the
registry the SCIM collectors went into - `None` when metrics are off, so there is nothing to
serve.

The endpoint sits outside the SCIM scope, next to `/health`, so a scraper reaches it without a
bearer token and without asking for `application/scim+json` - **keeping it off a public interface
is the operator's job**. Pass your own registry instead, and a service's own metrics are served
from the same endpoint:

```rust
let registry = prometheus::Registry::new();  // scimd::MetricsRegistry
.metrics_registry(registry.clone())          // implies with_metrics(true)
```

The SCIM collectors can only be registered once per registry, so a host that mounts scimd twice
against one registry gets the series from the first mount; the second logs a warning and serves its
routes without them.

| Metric | Labels |
|---|---|
| `scimd_http_requests_total` | `endpoint`, `method`, `status` |
| `scimd_http_request_duration_seconds` | `endpoint`, `method`, `status` |
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

## kimaid

A sidecar that puts SCIM in front of [Kimai](https://www.kimai.org/), talking to it over Kimai's
own REST API (`kimai-openapi.json`). SCIM `/Users` are Kimai users and SCIM `/Groups` are Kimai
teams.

```
KIMAI_URL=https://kimai.example.com \
KIMAI_API_TOKEN=... \
SERVICE_BEARER_TOKEN=... \
  cargo run -p kimaid -- --bind 127.0.0.1:5001
```

Secrets are environment variables because `scimd_server::load_config` logs the config file it
read. `config.toml` carries only what Kimai requires of every user and SCIM need not send:

```toml
language = "de"          # Kimai's `language` enum
locale = "de_DE"         # Kimai's `locale` enum, regions spelled with an underscore
timezone = "Europe/Berlin"
```

A SCIM `id` is the Kimai id it names, so `/v2/Users/42` is Kimai's user 42. Kimai has nowhere to
keep an `externalId`, so kimaid accepts one and drops it. A client that lost its mapping
re-correlates with `?filter=userName eq "..."`.

Two limits come from Kimai's API, and the client sees both:

- **A user cannot be deleted.** Kimai has no such endpoint, and a time-tracking system should keep
  the timesheets attached to an account, so `DELETE /Users/{id}` deactivates. `active: false` does
  the same, so both deprovisioning routes agree.
- **A user cannot be renamed.** Kimai's user edit form has no `username`, so a `userName` change
  draws a `400 invalidValue` instead of being dropped without a word.

kimaid never writes Kimai roles. Sending `roles` replaces a user's entire role list, and the valid
values differ per instance, so omitting the field leaves Kimai's own roles alone.

Neither Kimai collection endpoint pages or filters, so kimaid fetches both whole and hands them to
`scimd::filter::list_response`, which applies `filter` and then slices by `startIndex`/`count`.

## Directory structure

```
example/         # bin example: the standalone server
example-mounted/ # bin example: scimd mounted into a host's own app, on `scimd` alone
gitlabd/         # microservice that creates users in gitlab on request
kimaid/          # microservice that provisions users and teams into kimai
scimd/           # the actix-web adapter: routes, middleware, App/AppBuilder/configure
scimd-core/      # Scim: the registry, dispatch, /Bulk, discovery, filters and SCIM metrics
scimd-server/    # the daemon: sockets, signals, --flags, config file, logging, /health, /metrics
scimd-types/     # the data model, the Error a service answers with, and the Resource trait
```

Only `scimd` and `scimd-server` know what a web framework is, and only `scimd-server` knows what a
*process* is, so a host that mounts `scimd` into an `HttpServer` it already runs compiles none of
the daemon. `scimd` re-exports everything below it - `scimd::models::User`, `scimd::Error` and
`scimd::filter::parse` are the paths to write, whichever crate they live in.

One exception: `scimd::Error` cannot implement actix's `ResponseError` from `scimd-types`.
`scimd::ScimError` wraps it and does, and `?` converts, so a hand-written actix handler returns
`Result<_, scimd::ScimError>`.

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

## License

AGPL-3.0-or-later, see [LICENSE](LICENSE).
