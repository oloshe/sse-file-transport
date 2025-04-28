FROM rust:1.86.0

# 设置 Rust 的国内镜像源 (中科大源)
RUN echo '[source.crates-io]' >> /usr/local/cargo/config && \
    echo 'replace-with = "ustc"' >> /usr/local/cargo/config && \
    echo '[source.ustc]' >> /usr/local/cargo/config && \
    echo 'registry = "https://mirrors.ustc.edu.cn/crates.io-index"' >> /usr/local/cargo/config

# 设置 apt 使用国内镜像源 (阿里云源)
RUN sed -i 's/deb.debian.org/mirrors.aliyun.com/g' /etc/apt/sources.list && \
    sed -i 's/security.debian.org/mirrors.aliyun.com/g' /etc/apt/sources.list

# 更新并安装必要的工具
RUN apt-get update && apt-get install -y \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

# 设置工作目录
WORKDIR /usr/src/app

COPY . .
RUN cargo build --release
CMD ["./target/release/sse-file-transport"]