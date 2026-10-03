FROM rust:1.85-slim AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked
FROM rust:1.85-slim
RUN apt-get update && apt-get install -y --no-install-recommends git ca-certificates procps python3 && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/mutarust /usr/local/bin/mutarust
RUN git config --global user.email et@example.com && git config --global user.name et && git config --global init.defaultBranch main
WORKDIR /work
