# ZEC SSL coordinator and operator node. One image, two binaries:
#   docker run ... ssl-coordinator
#   docker run ... ssl-operator
# Build context: the protocol/ directory (the Cargo workspace).

FROM rust:1.93-bookworm AS build
WORKDIR /src
COPY . .
# Only the services; the FROST spike tools are not part of the image.
RUN cargo build --release --locked -p ssl-node \
 && cp target/release/ssl-coordinator target/release/ssl-operator /usr/local/bin/

# SQLite is bundled and TLS roots are compiled in, so glibc is all the runtime needs.
FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/bin/ssl-coordinator /usr/local/bin/ssl-operator /usr/local/bin/

# State (orders, approvals, the view-only wallet) lives on a persistent disk mounted here.
# The services run as root because hosted persistent disks (Render) are mounted root-owned.
ENV STATE_DIR=/var/data \
    RUST_LOG=info
VOLUME ["/var/data"]
CMD ["ssl-operator"]
