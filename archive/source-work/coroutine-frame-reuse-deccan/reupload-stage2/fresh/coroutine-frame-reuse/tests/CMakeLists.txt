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
project(roundel_out_of_process_tests CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

if(NOT CMAKE_BUILD_TYPE)
    set(CMAKE_BUILD_TYPE Debug)
endif()

option(BUILD_CANDIDATE "Build the isolated candidate scenario process" OFF)

if(BUILD_CANDIDATE)
    set(FROZEN_DIR "${CMAKE_CURRENT_SOURCE_DIR}/frozen")
    set(CANDIDATE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/candidate")
    add_executable(roundel_candidate
        "${CMAKE_CURRENT_SOURCE_DIR}/scenario.cpp"
        "${FROZEN_DIR}/src/journal.cpp"
        "${FROZEN_DIR}/src/arena.cpp"
        "${CANDIDATE_DIR}/src/runtime.cpp"
    )
    # System-header provenance is intentionally absent from the preprocessed
    # scenario translation unit; suppress warnings for that trusted generated
    # file only. Submitted runtime.cpp remains under the strict target flags.
    set_source_files_properties("${CMAKE_CURRENT_SOURCE_DIR}/scenario.cpp"
        PROPERTIES COMPILE_OPTIONS "-w")
    target_include_directories(roundel_candidate PRIVATE
        "${FROZEN_DIR}/include" "${CANDIDATE_DIR}/include")
    target_compile_options(roundel_candidate PRIVATE
        -Wall -Wextra -Wpedantic -Werror
        -Wno-unused-but-set-variable -Wno-unused-const-variable
        -fsanitize=address,undefined -fno-omit-frame-pointer -g)
    target_link_options(roundel_candidate PRIVATE -fsanitize=address,undefined)
else()
    # This target includes no Roundel header and links no submitted object.
    add_executable(roundel_verifier
        "${CMAKE_CURRENT_SOURCE_DIR}/verifier_driver.cpp")
    target_compile_options(roundel_verifier PRIVATE
        -Wall -Wextra -Wpedantic -Werror)
endif()
