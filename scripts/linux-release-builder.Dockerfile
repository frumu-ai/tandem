# The Rust toolchain and Ubuntu 22.04 userland are independently immutable.
FROM rust:1.95.0-bullseye@sha256:646e8ceea789b00c5cfa339816a3ed44940dbf1651dc167b78f3c0aefcae0025 AS rust_toolchain

FROM buildpack-deps:jammy@sha256:0704d9775531e89274dca865a6bdaf13ed71a64bfe36f3a01cf6bd59bdf1f6eb

COPY --from=rust_toolchain /usr/local/cargo /usr/local/cargo
COPY --from=rust_toolchain /usr/local/rustup /usr/local/rustup

ENV CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
