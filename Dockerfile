FROM rust:1.74

RUN apt-get update && apt-get install -y \
  ffmpeg \
  && rm -rf /var/lib/apt/lists/*
WORKDIR /usr/src/timelapse-service-rs
COPY . .

RUN cargo install --path .

CMD ["timelapse-service-rs"]
