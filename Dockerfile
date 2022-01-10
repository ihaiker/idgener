FROM rust:1.57.0-alpine3.14 AS builder
COPY . /build
WORKDIR /build
RUN apk add --no-cache make build-base openssl-dev
RUN cargo install --path . && cargo build --release

FROM alpine:3.14
COPY --from=builder /build/target/release/idgener /apps/idgener
COPY etc /apps/etc

WORKDIR /apps
EXPOSE  7656
ENTRYPOINT ["/apps/idgener"]
