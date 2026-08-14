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
