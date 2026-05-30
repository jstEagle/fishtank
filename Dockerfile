FROM rust:1.95-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --locked --release -p fishtank-server

FROM debian:bookworm-slim
WORKDIR /app
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/* \
  && mkdir -p /data/fishtank
COPY --from=builder /app/target/release/fishtank-server /usr/local/bin/fishtank-server
COPY config ./config
COPY worlds ./worlds
CMD ["sh", "-lc", ". /app/config/fishtank.defaults.env && exec fishtank-server serve --world worlds/village.json --state /data/fishtank"]
