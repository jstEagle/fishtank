FROM rust:1.95-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p fishtank-server

FROM debian:bookworm-slim
WORKDIR /app
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/fishtank-server /usr/local/bin/fishtank-server
COPY worlds ./worlds
ENV RUST_LOG=fishtank_server=info,tower_http=info
CMD ["fishtank-server", "serve"]
