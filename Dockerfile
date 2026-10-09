FROM rust:1.97-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --locked --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/kube-bug-agent /usr/local/bin/kube-bug-agent
COPY --from=builder /build/target/release/backpressure-agent /usr/local/bin/backpressure-agent
USER 10001:10001
ENTRYPOINT ["kube-bug-agent"]
