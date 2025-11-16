ARG SERVICE_NAME

FROM lukemathwalker/cargo-chef:latest-rust-1.88-slim-bookworm as chef
WORKDIR /services

# Prepare dependency graph
FROM chef as planner
ARG SERVICE_NAME
ENV SERVICE_NAME=${SERVICE_NAME}
COPY . .
RUN cargo chef prepare --bin ${SERVICE_NAME} --recipe-path recipe.json

# Build the dependencies
FROM chef as builder
ARG SERVICE_NAME
ENV SERVICE_NAME=${SERVICE_NAME}
COPY --from=planner /services/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --bin ${SERVICE_NAME}

# Build the binary
COPY . .
RUN cargo build --release --bin ${SERVICE_NAME}

# Run the service
FROM debian:bookworm-slim AS runtime
ARG SERVICE_NAME
COPY --from=builder /services/target/release/${SERVICE_NAME} /usr/local/bin/main
ENTRYPOINT ["/usr/local/bin/main"]
