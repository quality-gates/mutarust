# Runtime image: docker build -t mutarust . && docker run --rm -v "$PWD":/code -w /code mutarust .
FROM rust:1.85-slim AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked

FROM rust:1.85-slim
RUN apt-get update && apt-get install -y --no-install-recommends git ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/mutarust /usr/local/bin/mutarust
WORKDIR /code
ENTRYPOINT ["mutarust"]
CMD ["--help"]
