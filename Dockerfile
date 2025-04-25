FROM rust:1.8.6
WORKDIR /app
COPY . .
RUN cargo build --release
CMD ["./target/release/your_rust_app"]