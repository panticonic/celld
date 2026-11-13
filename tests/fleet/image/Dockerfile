# Runtime image for celld fleet nodes. The celld binary is bind-mounted at run
# time (CELLD_BIN), so this image only provides a glibc >= 2.39 userland plus the
# fault-injection tools used through `docker exec`: iptables (partitions) and
# tc (netem, slow bucket).
FROM ubuntu:24.04
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates iptables iproute2 procps curl && \
    rm -rf /var/lib/apt/lists/*
