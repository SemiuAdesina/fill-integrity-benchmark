#
# Task image.
#
# The base image already carries clang, cmake, make and python3, so nothing is
# installed here. There is no apt, no apk, no curl, no wget and no network use at
# any point in this build.
#
# The grading suite lives in the separate verifier image. Nothing under this
# image's /app is a test, an expected output or a reference solution.

FROM silkeh/clang@sha256:45c29eadcae5f9817d0d7dc8518a0cb2a565647e7e049656d1c0bc6e9d9649d3

ENV CMAKE_COLOR_DIAGNOSTICS=OFF
ENV ROUNDEL_HOME=/app

ENTRYPOINT []

WORKDIR /app

COPY CMakeLists.txt /app/CMakeLists.txt
COPY include /app/include
COPY src /app/src
COPY examples /app/examples
COPY diagnostics /app/diagnostics
COPY spec /app/spec
COPY README.md /app/README.md

# Warm the build so the first thing an agent does is not a cold configure. The
# build directory is left in place; it is regenerated from scratch by the
# verifier in any case.
RUN cmake -S /app -B /app/build -DCMAKE_BUILD_TYPE=Debug \
    && cmake --build /app/build -j "$(nproc)"

CMD ["sleep", "infinity"]
