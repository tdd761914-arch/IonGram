FROM rust:bookworm AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends python3 ca-certificates && rm -rf /var/lib/apt/lists/*
COPY . .
RUN python3 tools/fetch_schemas.py && cargo build --release --locked
FROM debian:bookworm-slim
RUN useradd --uid 10001 --create-home iongram && mkdir /data && chown iongram:iongram /data
COPY --from=builder /app/target/release/iongram /usr/local/bin/iongram
USER iongram
ENV IONGRAM_DATA=/data IONGRAM_BIND=0.0.0.0:4430
VOLUME /data
EXPOSE 4430
ENTRYPOINT ["iongram"]
CMD ["serve"]
