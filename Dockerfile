FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p rcli-server

FROM debian:bookworm-slim
COPY --from=build /src/target/release/rcli-server /usr/local/bin/rcli-server
EXPOSE 8080 8443
CMD ["rcli-server"]
