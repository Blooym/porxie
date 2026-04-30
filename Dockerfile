# ----------
#   SETUP
# ----------
FROM alpine:latest AS setup
RUN adduser -S -s /bin/false -D porxie

# -----------
#    BUILD
# -----------
FROM rust:1-alpine AS build
WORKDIR /build
RUN apk add --no-cache --update build-base

# Pre-cache dependencies
COPY Cargo.toml Cargo.lock ./
COPY crates/porxie/Cargo.toml crates/porxie/Cargo.toml
COPY crates/lexgen/Cargo.toml crates/lexgen/Cargo.toml
RUN mkdir -p crates/porxie/src crates/lexgen/src \
    && echo "// Placeholder" > crates/porxie/src/lib.rs \
    && echo "// Placeholder" > crates/lexgen/src/lib.rs \
    && cargo build --release \
    && rm crates/porxie/src/lib.rs crates/lexgen/src/lib.rs

# Build
COPY crates ./crates
RUN cargo build --release

# -----------
#   RUNTIME
# -----------
FROM scratch
WORKDIR /opt

COPY --from=build /build/target/release/porxie /usr/bin/porxie
COPY --from=setup /etc/passwd /etc/passwd
COPY --from=setup /bin/false /bin/false
USER porxie

# Set configuration defaults for container builds.
ENV PORXIE_SERVER_ADDRESS=ip:0.0.0.0:6314
ENV RUST_LOG=info
EXPOSE 6314

ENTRYPOINT ["/usr/bin/porxie"]
