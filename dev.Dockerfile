# Development image: docker build -f dev.Dockerfile -t mutarust-dev . && docker run --rm -it -v "$PWD":/workspace mutarust-dev
FROM rust:1.85-slim
RUN apt-get update && apt-get install -y --no-install-recommends git bash && rm -rf /var/lib/apt/lists/*
WORKDIR /workspace
COPY . .
CMD ["cargo", "test"]
