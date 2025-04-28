FROM rust:1.86.0
WORKDIR /app
COPY . .
RUN cargo build --release
CMD ["./target/release/sse-file-transport"]