# Clinilog API — production image for Render.
#
# Build stage: compiles both binaries (the API server and the one-time `seed`).
FROM rust:1-slim-trixie AS build
WORKDIR /app

# Cache dependencies: build them once against a stub, so code-only changes rebuild fast.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src/bin \
    && echo 'fn main() {}' > src/main.rs \
    && echo 'fn main() {}' > src/bin/seed.rs \
    && touch src/lib.rs \
    && cargo build --release --locked \
    && rm -rf src target/release/.fingerprint/clinilog-server-*

COPY build.rs ./
COPY migrations ./migrations
COPY src ./src
RUN cargo build --release --locked

# Runtime stage: a small Debian image with just the binaries and CA certificates.
FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --no-create-home clinilog
COPY --from=build /app/target/release/clinilog-server /app/target/release/seed /usr/local/bin/
USER clinilog
ENV RUST_LOG=clinilog_server=info,tower_http=info
# Render sets PORT; the server listens on 0.0.0.0:$PORT (8080 if unset).
EXPOSE 8080
CMD ["clinilog-server"]
