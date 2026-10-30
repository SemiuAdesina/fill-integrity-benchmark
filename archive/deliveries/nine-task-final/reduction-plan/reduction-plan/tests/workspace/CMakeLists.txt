#
# Verifier-owned build manifest.
#
# No candidate manifest, cmake config, toolchain file, build script or prebuilt
# binary is ever read, and nothing here refers to /app: the verifier copies
# exactly one allowlisted candidate source into `candidate/recover.cpp` and
# builds it with this file, using an absolute compiler passed as
# -DCMAKE_CXX_COMPILER.
#
# Three targets, and the separation between them is the whole trust model:
#
#   probe       frozen contract + shared harness + THE CANDIDATE implementation.
#               Records raw functional values. Grades nothing.
#   liar_probe  the same harness translation unit, byte for byte, built against
#               a verifier-owned deliberately-wrong implementation. Used only by
#               the stdout-authority regression.
#   judge       expectations only. Never compiles or links the candidate
#               implementation or the wrong one, so no candidate code shares its
#               address space and no candidate output reaches it.
cmake_minimum_required(VERSION 3.16)
project(fabric_verifier LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# Same diagnostics the V1 harness built the candidate under.
set(FABRIC_WARNINGS -Wall -Wextra -Wpedantic -Werror)

# Same sanitizers V1 built the candidate under. They apply to the two probes,
# which are the binaries that link an implementation of the contract; the judge
# is built without them so that no sanitizer exit status can perturb the
# grading process, whose exit code gates reward.
set(FABRIC_SANITIZERS -fsanitize=address,undefined -fno-omit-frame-pointer)

set(FABRIC_FROZEN_INCLUDE ${CMAKE_CURRENT_SOURCE_DIR}/frozen/include)
set(FABRIC_SHARED_INCLUDE ${CMAKE_CURRENT_SOURCE_DIR}/shared)
set(FABRIC_HARNESS_SOURCE ${CMAKE_CURRENT_SOURCE_DIR}/probe/main.cpp)

# The frozen public contract, identical for every target. It is header-only:
# `saturating_add`, `replay` and `recover` are only declared here, so the judge
# links cleanly without any implementation of them anywhere in its binary.
add_library(fabric_frozen INTERFACE)
target_include_directories(fabric_frozen INTERFACE ${FABRIC_FROZEN_INCLUDE})

# Header-only shared layer: fixtures, record codec, digest, observation harness.
# It contains inputs and serialization only -- never an expected result -- which
# is why it is safe to compile into a binary the graded account executes.
add_library(fabric_shared INTERFACE)
target_include_directories(fabric_shared INTERFACE ${FABRIC_SHARED_INCLUDE})

# The graded probe. `candidate/recover.cpp` is placed here by the verifier from
# the single allowlisted path; it is the only candidate-authored file in the
# build.
add_executable(probe ${FABRIC_HARNESS_SOURCE} ${CMAKE_CURRENT_SOURCE_DIR}/candidate/recover.cpp)
target_link_libraries(probe PRIVATE fabric_frozen fabric_shared)
target_compile_options(probe PRIVATE ${FABRIC_WARNINGS} ${FABRIC_SANITIZERS})
target_link_options(probe PRIVATE ${FABRIC_SANITIZERS})

# The stdout-authority regression probe. Identical harness translation unit,
# different implementation -- so the two probes cannot drift apart.
add_executable(liar_probe ${FABRIC_HARNESS_SOURCE} ${CMAKE_CURRENT_SOURCE_DIR}/liar/recover.cpp)
target_link_libraries(liar_probe PRIVATE fabric_frozen fabric_shared)
target_compile_options(liar_probe PRIVATE ${FABRIC_WARNINGS} ${FABRIC_SANITIZERS})
target_link_options(liar_probe PRIVATE ${FABRIC_SANITIZERS})

# The sole grading authority. Note the source list: no candidate file, no liar
# file, no observation harness -- nothing but the judge itself.
add_executable(judge ${CMAKE_CURRENT_SOURCE_DIR}/judge/main.cpp)
target_link_libraries(judge PRIVATE fabric_frozen fabric_shared)
target_compile_options(judge PRIVATE ${FABRIC_WARNINGS})
