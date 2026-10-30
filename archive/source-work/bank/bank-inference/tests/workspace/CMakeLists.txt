#
# Verifier-owned build manifest.
#
# No candidate manifest, cmake config, toolchain file, build script or prebuilt
# binary is ever read: the verifier copies exactly one allowlisted candidate
# source into `candidate/recover.cpp` and builds it with this file, using an
# absolute compiler passed as -DCMAKE_CXX_COMPILER.
#
# Three targets, and the separation between them is the whole trust model:
#
#   probe       frozen interpreter + shared harness + THE CANDIDATE recovery.
#               Records raw functional values. Grades nothing.
#   liar_probe  the same harness translation unit, byte for byte, built against
#               a verifier-owned deliberately-wrong recovery. Used only by the
#               stdout-authority regression.
#   judge       frozen interpreter + expectations. Never compiles or links the
#               candidate recovery or the wrong one, so no candidate code shares
#               its address space and no candidate output reaches it.
cmake_minimum_required(VERSION 3.16)
project(bank_verifier LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# Same diagnostics the V1 harness built the candidate under.
set(BANK_WARNINGS -Wall -Wextra -Wpedantic -Werror)

set(BANK_FROZEN_INCLUDE ${CMAKE_CURRENT_SOURCE_DIR}/frozen/include)
set(BANK_SHARED_INCLUDE ${CMAKE_CURRENT_SOURCE_DIR}/shared)
set(BANK_FROZEN_SOURCE ${CMAKE_CURRENT_SOURCE_DIR}/frozen/src/model.cpp)
set(BANK_HARNESS_SOURCE ${CMAKE_CURRENT_SOURCE_DIR}/probe/main.cpp)

# The frozen interpreter, identical for every target. It defines replay and
# canonical; bank::recover is only declared in the frozen header, so the judge
# links cleanly without any implementation of it.
add_library(frozen_model STATIC ${BANK_FROZEN_SOURCE})
target_include_directories(frozen_model PUBLIC ${BANK_FROZEN_INCLUDE})
target_compile_options(frozen_model PRIVATE ${BANK_WARNINGS})

# Header-only shared layer: fixtures, record codec, observation harness. It
# contains inputs and serialization only -- never an expected result -- which is
# why it is safe to compile into a binary the graded account executes.
add_library(bank_shared INTERFACE)
target_include_directories(bank_shared INTERFACE ${BANK_SHARED_INCLUDE})

# The graded probe. `candidate/recover.cpp` is placed here by the verifier from
# the single allowlisted path; it is the only candidate-authored file in the
# build.
add_executable(probe ${BANK_HARNESS_SOURCE} ${CMAKE_CURRENT_SOURCE_DIR}/candidate/recover.cpp)
target_link_libraries(probe PRIVATE frozen_model bank_shared)
target_compile_options(probe PRIVATE ${BANK_WARNINGS})

# The stdout-authority regression probe. Identical harness translation unit,
# different recovery -- so the two probes cannot drift apart.
add_executable(liar_probe ${BANK_HARNESS_SOURCE} ${CMAKE_CURRENT_SOURCE_DIR}/liar/recover.cpp)
target_link_libraries(liar_probe PRIVATE frozen_model bank_shared)
target_compile_options(liar_probe PRIVATE ${BANK_WARNINGS})

# The sole grading authority. Note the source list: no candidate file, no liar
# file, nothing but the judge and the frozen interpreter.
add_executable(judge ${CMAKE_CURRENT_SOURCE_DIR}/judge/main.cpp)
target_link_libraries(judge PRIVATE frozen_model bank_shared)
target_compile_options(judge PRIVATE ${BANK_WARNINGS})
