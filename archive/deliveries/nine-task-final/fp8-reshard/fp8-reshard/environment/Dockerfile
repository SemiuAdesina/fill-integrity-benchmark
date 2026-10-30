FROM silkeh/clang@sha256:45c29eadcae5f9817d0d7dc8518a0cb2a565647e7e049656d1c0bc6e9d9649d3
ENTRYPOINT []
WORKDIR /app
COPY CMakeLists.txt /app/CMakeLists.txt
COPY include /app/include
COPY src /app/src
COPY examples /app/examples
COPY spec /app/spec
COPY README.md /app/README.md
RUN cmake -S /app -B /app/build -DCMAKE_BUILD_TYPE=Debug && cmake --build /app/build -j 2
CMD ["sleep","infinity"]
