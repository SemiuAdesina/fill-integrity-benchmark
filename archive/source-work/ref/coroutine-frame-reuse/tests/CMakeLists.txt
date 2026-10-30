#
# Verifier-owned build configuration.
#
# The verifier copies exactly two candidate sources from /app:
#   /app/include/roundel/runtime.hpp
#   /app/src/runtime.cpp
# and rebuilds the project from those two files plus pristine copies of the
# frozen contract and its own hidden tests. The candidate's CMakeLists.txt is
# not used.

cmake_minimum_required(VERSION 3.20)
project(roundel_tests CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

if(NOT CMAKE_BUILD_TYPE)
    set(CMAKE_BUILD_TYPE Debug)
endif()

# Pristine frozen sources baked into the verifier image at /tests/frozen.
set(FROZEN_DIR "${CMAKE_CURRENT_SOURCE_DIR}/frozen")

# Candidate sources copied by test.sh into /verifier/candidate.
set(CANDIDATE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/candidate")

# Build the runtime as a static library from one pristine and one candidate file.
add_library(roundel_runtime STATIC
    "${FROZEN_DIR}/src/journal.cpp"
    "${FROZEN_DIR}/src/arena.cpp"
    "${CANDIDATE_DIR}/src/runtime.cpp"
)

target_include_directories(roundel_runtime PUBLIC
    "${FROZEN_DIR}/include"
    "${CANDIDATE_DIR}/include"
)

# Strict warnings, always.
target_compile_options(roundel_runtime PRIVATE
    -Wall -Wextra -Wpedantic -Werror
)

# Sanitizers. ASAN_OPTIONS, UBSAN_OPTIONS and LSAN_OPTIONS are set by test.sh
# to trusted values; the candidate cannot influence them.
target_compile_options(roundel_runtime PUBLIC
    -fsanitize=address,undefined
    -fno-omit-frame-pointer
    -g
)
target_link_options(roundel_runtime PUBLIC
    -fsanitize=address,undefined
)

# The graded executable.
add_executable(roundel_tests
    "${CMAKE_CURRENT_SOURCE_DIR}/graded.cpp"
)

target_link_libraries(roundel_tests PRIVATE roundel_runtime)

target_compile_options(roundel_tests PRIVATE
    -Wall -Wextra -Wpedantic -Werror
)
