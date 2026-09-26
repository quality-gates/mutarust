FROM mutarust-et:406beab
RUN rustup component add llvm-tools-preview && cargo install cargo-llvm-cov --version 0.6.16 --locked && rm -rf /usr/local/cargo/registry
