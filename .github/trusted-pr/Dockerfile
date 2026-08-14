FROM ubuntu@sha256:019e8eb29a85e74d64925745884f2ec79aa27e3feab36353d24656f4d6b89467

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        bash build-essential ca-certificates clang cmake curl git jq libssl-dev \
        pkg-config protobuf-compiler python3 python3-venv ripgrep shellcheck unzip xz-utils \
        fonts-liberation libasound2t64 libatk-bridge2.0-0 libatk1.0-0 libcups2 \
        libdbus-1-3 libdrm2 libgbm1 libglib2.0-0 libnspr4 libnss3 libpango-1.0-0 \
        libx11-6 libxcb1 libxcomposite1 libxdamage1 libxext6 libxfixes3 \
        libxkbcommon0 libxrandr2 \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd --gid 10001 bridge-ci \
    && useradd --uid 10001 --gid 10001 --create-home --shell /bin/bash bridge-ci

USER 10001:10001
WORKDIR /workspace
