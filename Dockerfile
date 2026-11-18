# syntax=docker/dockerfile:1
FROM node:24-alpine AS frontend
WORKDIR /build/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1-alpine AS backend
RUN apk add --no-cache build-base
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
ENV RUSTFLAGS="-C target-feature=+crt-static"
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked --target x86_64-unknown-linux-musl && \
    mkdir -p /out && cp target/x86_64-unknown-linux-musl/release/homeclip /out/homeclip

FROM scratch AS runtime
LABEL org.opencontainers.image.title="Homeclip" \
      org.opencontainers.image.source="https://github.com/sgqy/homeclip" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.version="0.1.0"
COPY --from=backend /out/homeclip /homeclip
COPY --from=frontend /build/web/dist /web
ENV HOMECLIP_DATA=/data HOMECLIP_WEB=/web HOMECLIP_BIND=0.0.0.0:8080 SQLITE_TMPDIR=/data
VOLUME ["/data"]
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 CMD ["/homeclip", "--healthcheck"]
ENTRYPOINT ["/homeclip"]
