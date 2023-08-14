FROM rust:1.71-slim AS builder

RUN apt-get update && apt-get install -y libsodium-dev pkg-config

COPY . /sources
WORKDIR /sources
RUN cargo build --release
RUN chown nobody:nogroup /sources/target/release/pisshoff-server

FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y libsodium23 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /sources/target/release/pisshoff-server /pisshoff-server
COPY --from=builder /sources/pisshoff-server/config.toml /config.toml

RUN touch audit.jsonl && chown nobody audit.jsonl

USER nobody
EXPOSE 2233
ENTRYPOINT ["/pisshoff-server", "-c", "/config.toml"]
