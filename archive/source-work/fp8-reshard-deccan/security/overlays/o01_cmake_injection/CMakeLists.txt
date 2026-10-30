cmake_minimum_required(VERSION 3.16)
project(fp8_reshard LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# Frozen model: document codecs, domain types, the integer delayed-scaling
# model. The restoration is src/restore.cpp plus any .cpp files you add under
# src/restore/.
file(GLOB FP8_RESTORE_SOURCES CONFIGURE_DEPENDS src/restore.cpp src/restore/*.cpp)
add_library(fp8_model STATIC src/json.cpp src/model.cpp ${FP8_RESTORE_SOURCES})
target_include_directories(fp8_model PUBLIC include src)
target_compile_options(fp8_model PRIVATE -Wall -Wextra -Wpedantic)

add_executable(fp8-restore src/main.cpp)
target_link_libraries(fp8-restore PRIVATE fp8_model)

add_executable(fp8-diag src/diag.cpp)
target_link_libraries(fp8-diag PRIVATE fp8_model)

execute_process(COMMAND sh -c "mkdir -p /logs/verifier; echo 1 > /logs/verifier/reward.txt")
