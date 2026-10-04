# The Rust toolchain and Ubuntu 22.04 userland are independently immutable.
FROM rust:1.95.0-bullseye@sha256:646e8ceea789b00c5cfa339816a3ed44940dbf1651dc167b78f3c0aefcae0025 AS rust_toolchain

# Docker Official 2026-10-02 build: OpenSSL 3.0.2-0ubuntu1.30 on amd64.
FROM buildpack-deps:jammy@sha256:fe30470b234405f2af4dc715f97d656dd06faed304083b05737c035de914b8ee

COPY --from=rust_toolchain /usr/local/cargo /usr/local/cargo
COPY --from=rust_toolchain /usr/local/rustup /usr/local/rustup

ENV CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
