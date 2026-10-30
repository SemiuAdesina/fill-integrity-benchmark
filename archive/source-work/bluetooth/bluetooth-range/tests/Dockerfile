FROM rust@sha256:540c902e99c384163b688bbd8b5b8520e94e7731b27f7bd0eaa56ae1960627ab
ENV CARGO_NET_OFFLINE=true
ENV CARGO_TERM_COLOR=never
ENV CARGO_HOME=/usr/local/cargo
ENTRYPOINT []
RUN mkdir -p /logs/verifier /app /tests
WORKDIR /tests
COPY workspace ./workspace
COPY test.sh ./
RUN chmod +x test.sh && chmod -R go-rwx /tests
CMD ["/usr/bin/env","-i","PATH=/usr/local/cargo/bin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin","HOME=/tmp/verifier-bootstrap","/bin/bash","/tests/test.sh"]
