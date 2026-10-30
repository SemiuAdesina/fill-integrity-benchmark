FROM pc-agent:reup-base
COPY refcrate /lab/refcrate
RUN cd /lab/refcrate && rm -rf target && /usr/local/cargo/bin/cargo build --release --offline 2>&1 | tail -2
