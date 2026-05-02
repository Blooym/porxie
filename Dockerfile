# Build Porxie.
FROM rust:1-alpine AS build
WORKDIR /build
RUN apk add --no-cache --update build-base

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release && \
    cp target/release/porxie /build/porxie


# Environment to steal some files from.
FROM alpine:latest AS setup
RUN adduser -S -s /bin/false -D porxie


# Runtime
FROM scratch
WORKDIR /opt

COPY --from=build /build/porxie /usr/bin/porxie
COPY --from=setup /etc/passwd /etc/passwd
COPY --from=setup /bin/false /bin/false
USER porxie

# Set configuration defaults for container builds.
ENV PORXIE_SERVER_ADDRESS=ip:0.0.0.0:6314
ENV RUST_LOG=info
EXPOSE 6314

ENTRYPOINT ["/usr/bin/porxie"]
