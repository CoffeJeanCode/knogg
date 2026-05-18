FROM rust:1-bookworm

WORKDIR /app

COPY Cargo.toml Cargo.lock* ./
COPY src ./src

RUN cargo build --release

ENTRYPOINT ["./target/release/knogg"]
