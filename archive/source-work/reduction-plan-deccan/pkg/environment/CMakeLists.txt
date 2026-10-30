cmake_minimum_required(VERSION 3.16)
project(reduction_plan LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# Frozen: document codecs and domain types. The recovery is src/recover.cpp
# plus any .cpp files you add under src/recover/.
file(GLOB FABRIC_RECOVER_SOURCES CONFIGURE_DEPENDS src/recover.cpp src/recover/*.cpp)
add_library(fabric_model STATIC src/json.cpp src/model.cpp ${FABRIC_RECOVER_SOURCES})
target_include_directories(fabric_model PUBLIC include src)
target_compile_options(fabric_model PRIVATE -Wall -Wextra -Wpedantic)

add_executable(reduction-recover src/main.cpp)
target_link_libraries(reduction-recover PRIVATE fabric_model)

add_executable(reduction-diag src/diag.cpp)
target_link_libraries(reduction-diag PRIVATE fabric_model)
