# Replacement for the cFE core module "msg".
#
# The cFS build finds this directory before cfe/modules/msg when CFS_APP_PATH names the parent
# directory. It builds the stock module unchanged, then moves the functions listed in
# replaced.cmake from the C implementation to the Rust library.

# Link a target that another directory created (the stock module's).
cmake_policy(SET CMP0079 NEW)

if (CMAKE_CROSSCOMPILING)
    message(FATAL_ERROR "cfs-rs: cross builds are not set up yet (no Rust target mapping)")
endif()

get_filename_component(CFS_RS_ROOT "${CMAKE_CURRENT_LIST_DIR}/../.." ABSOLUTE)
set(CFS_RS_STOCK_MSG "${MISSION_SOURCE_DIR}/cfe/modules/msg")

include("${CMAKE_CURRENT_LIST_DIR}/replaced.cmake")

# The renames are compile options, not compile definitions: the cFS coverage-test function
# copies the module's compile definitions to the test cases, which must keep the real names.
set(CFS_RS_RENAMES)
set(CFS_RS_FORCE_LINK)
foreach(FN ${CFE_MSG_RS_REPLACED})
    list(APPEND CFS_RS_RENAMES "-D${FN}=${FN}_c")
    list(APPEND CFS_RS_FORCE_LINK "-Wl,--undefined=${FN}")
endforeach()

# The Rust static library. Cargo decides what is out of date, so the target always runs.
find_program(CARGO cargo REQUIRED)
set(CFS_RS_CARGO_DIR "${CMAKE_CURRENT_BINARY_DIR}/cargo")
set(CFS_RS_LIB "${CFS_RS_CARGO_DIR}/release/libcfe_msg_ffi.a")
add_custom_target(cfe_msg_rs_build
    COMMAND "${CARGO}" build --locked --release --package cfe-msg-ffi
            --manifest-path "${CFS_RS_ROOT}/Cargo.toml"
            --target-dir "${CFS_RS_CARGO_DIR}"
    BYPRODUCTS "${CFS_RS_LIB}"
    COMMENT "cargo build cfe-msg-ffi"
    VERBATIM
)
add_library(cfe_msg_rs STATIC IMPORTED GLOBAL)
set_target_properties(cfe_msg_rs PROPERTIES IMPORTED_LOCATION "${CFS_RS_LIB}")
add_dependencies(cfe_msg_rs cfe_msg_rs_build)

# The stock module, from its own CMakeLists.txt: target ${DEP} and, with ENABLE_UNIT_TESTS,
# its coverage test.
add_subdirectory("${CFS_RS_STOCK_MSG}" stock)

target_compile_options(${DEP} PRIVATE ${CFS_RS_RENAMES})
target_link_libraries(${DEP} PRIVATE cfe_msg_rs)

# The stock unit tests compile the C sources a second time, into an object library. The test
# runner also links cFE's stub library, which has a stub of every CFE_MSG function, so the Rust
# library goes first on the link line with each replaced function requested by name.
if (TARGET coverage-${DEP}-ALL-object)
    target_compile_options(coverage-${DEP}-ALL-object PRIVATE ${CFS_RS_RENAMES})
    add_dependencies(coverage-${DEP}-ALL-testrunner cfe_msg_rs_build)
    target_link_options(coverage-${DEP}-ALL-testrunner PRIVATE
        ${CFS_RS_FORCE_LINK} "${CFS_RS_LIB}")
    # A link option is not a dependency: relink when the Rust library changes.
    set_property(TARGET coverage-${DEP}-ALL-testrunner APPEND PROPERTY LINK_DEPENDS "${CFS_RS_LIB}")
endif()
