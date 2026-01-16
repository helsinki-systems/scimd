# SCIMD

See example directory for usage.

Currently only used for testing, ask Simon or Janne for more information.

## Directory structure

```
example/ # bin example that logs the requests and returns 501
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
