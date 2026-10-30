FROM silkeh/clang@sha256:45c29eadcae5f9817d0d7dc8518a0cb2a565647e7e049656d1c0bc6e9d9649d3
ENTRYPOINT []
WORKDIR /app
COPY CMakeLists.txt /app/CMakeLists.txt
COPY include /app/include
COPY src /app/src
COPY spec /app/spec
COPY examples /app/examples
RUN cmake -S /app -B /app/build -DCMAKE_BUILD_TYPE=Debug && cmake --build /app/build -j "$(nproc)"
CMD ["sleep", "infinity"]
