FROM rust:1.74

WORKDIR /usr/src/timelapse-rs
COPY . .

RUN cargo install --path .

CMD ["timelapse-rs"]
