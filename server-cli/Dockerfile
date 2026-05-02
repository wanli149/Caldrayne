FROM rust:bookworm AS builder

ARG RUST_TOOLCHAIN=nightly-2025-09-08
ARG PROJECTNAME=server-cli

ENV DEBIAN_FRONTEND=noninteractive
ENV CALDRAYNE_GIT_VERSION=/0/0
ENV RUSTUP_DIST_SERVER=https://mirrors.ustc.edu.cn/rust-static
ENV RUSTUP_UPDATE_ROOT=https://mirrors.ustc.edu.cn/rust-static/rustup

RUN sed -i 's|http://deb.debian.org/debian|http://mirrors.ustc.edu.cn/debian|g; s|http://deb.debian.org/debian-security|http://mirrors.ustc.edu.cn/debian-security|g' /etc/apt/sources.list.d/debian.sources

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        binutils \
        build-essential \
        ca-certificates \
        cmake \
        git \
        lld \
        mold \
        pkg-config \
        python3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src

COPY . /src
# Vendor the patched specs checkout so Docker builds do not depend on a
# flaky GitHub fetch for an unadvertised revision during image creation.
COPY ./vendor/specs /tmp/specs
COPY ./vendor/portpicker-rs /tmp/portpicker-rs
COPY ./vendor/iced /tmp/iced

RUN sed -i 's|specs = { git = "https://github.com/amethyst/specs.git", rev = "4e2da1df29ee840baa9b936593c45592b7c9ae27" }|specs = { path = "/tmp/specs" }|' /src/Cargo.toml \
    && sed -i 's|portpicker = { git = "https://github.com/xMAC94x/portpicker-rs", rev = "df6b37872f3586ac3b21d08b56c8ec7cd92fb172" }|portpicker = { path = "/tmp/portpicker-rs" }|' /src/server/Cargo.toml \
    && sed -i 's|iced = { package = "iced_native", git = "https://github.com/Imberflur/iced", tag = "veldr-winit-0.28" }|iced = { package = "iced_native", path = "/tmp/iced/native" }|' /src/voxygen/Cargo.toml

RUN rustup toolchain install ${RUST_TOOLCHAIN} --profile minimal

RUN cargo +${RUST_TOOLCHAIN} build \
    --release \
    -p veldr-server-cli \
    --bin caldrayne-server-cli \
    --features default-publish


FROM debian:bookworm-slim AS runtime

# SIGUSR1 causes caldrayne-server-cli to initiate a graceful shutdown
LABEL com.centurylinklabs.watchtower.stop-signal="SIGUSR1"

ARG PROJECTNAME=server-cli

ENV DEBIAN_FRONTEND=noninteractive

# iproute2 and net-tools for diagnostic purposes
RUN sed -i 's|http://deb.debian.org/debian|http://mirrors.ustc.edu.cn/debian|g; s|http://deb.debian.org/debian-security|http://mirrors.ustc.edu.cn/debian-security|g' /etc/apt/sources.list.d/debian.sources

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        iproute2 \
        net-tools \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /opt/assets/common /opt/assets/server /opt/assets/world /opt/userdata

COPY --from=builder /src/target/release/caldrayne-server-cli /opt/caldrayne-server-cli
COPY --from=builder /src/assets/common /opt/assets/common
COPY --from=builder /src/assets/server /opt/assets/server
COPY --from=builder /src/assets/world /opt/assets/world

WORKDIR /opt

ENV RUST_BACKTRACE=full
ENV CALDRAYNE_USERDATA=/opt/userdata

EXPOSE 14004 14006/udp

ENTRYPOINT ["/opt/caldrayne-server-cli"]
