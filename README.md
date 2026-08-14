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
