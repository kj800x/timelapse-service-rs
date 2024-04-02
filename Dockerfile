FROM rust:1.74

WORKDIR /usr/src/timelapse-service-rs
COPY . .

RUN cargo install --path .

CMD ["timelapse-service-rs"]
