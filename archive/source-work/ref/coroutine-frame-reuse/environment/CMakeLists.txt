#
# Build for the roundel runtime and its visible demo.
#
# Offline only: no fetch, no download, no package installation.

cmake_minimum_required(VERSION 3.20)
project(roundel LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

if(NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE Debug)
endif()

add_library(roundel STATIC
  src/arena.cpp
  src/journal.cpp
  src/runtime.cpp
)
target_include_directories(roundel PUBLIC include)
target_compile_options(roundel PRIVATE -Wall -Wextra -Wpedantic)

add_executable(roundel_demo examples/demo.cpp)
target_link_libraries(roundel_demo PRIVATE roundel)
target_compile_options(roundel_demo PRIVATE -Wall -Wextra -Wpedantic)

option(ROUNDEL_SANITIZE "Build with AddressSanitizer and UndefinedBehaviorSanitizer" OFF)
if(ROUNDEL_SANITIZE)
  set(_roundel_san -fsanitize=address,undefined -fno-omit-frame-pointer -g)
  target_compile_options(roundel PUBLIC ${_roundel_san})
  target_link_options(roundel PUBLIC ${_roundel_san})
endif()
