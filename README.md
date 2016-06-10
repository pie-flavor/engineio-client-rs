# engineio-rs

An engine.io client library in Rust. Runs only on Rust
nightly at the moment.

[![Build Status](https://travis-ci.org/NeoLegends/engineio-rs.svg?branch=master)](https://travis-ci.org/NeoLegends/engineio-rs)

## Usage

To use `engineio-rs`, first add this to your Cargo.toml:

```toml
[dependencies.engineio]
git = "https://github.com/NeoLegends/engineio-rs"
```

Then, add this to your crate root:

```rust
extern crate engineio;
```