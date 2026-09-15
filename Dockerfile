# 构建阶段:bundled SQLite 免系统依赖
FROM rust:1-slim AS builder
WORKDIR /app
COPY crates ./crates
COPY Cargo.toml Cargo.lock ./
RUN cargo build --release -p i-rs-schedule

# 运行阶段
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates wget && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/i-rs-schedule /usr/local/bin/irs-schedule
ENV SCHEDULE_PORT=3000 SCHEDULE_DB=/data/schedule.db
VOLUME /data
EXPOSE 3000
CMD ["irs-schedule"]
