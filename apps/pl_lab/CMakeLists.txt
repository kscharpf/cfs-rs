project(PL_LAB C)

if (CMAKE_CROSSCOMPILING)
    message(FATAL_ERROR "cfs-rs: cross builds are not set up yet (no Rust target mapping)")
endif()

get_filename_component(CFS_RS_ROOT "${CMAKE_CURRENT_LIST_DIR}/../.." ABSOLUTE)

find_program(CARGO cargo REQUIRED)
set(CFS_RS_CARGO_DIR "${CMAKE_CURRENT_BINARY_DIR}/cargo")
set(CFS_RS_LIB "${CFS_RS_CARGO_DIR}/release/libpl_lab.a")

add_custom_target(pl_lab_rs_build
    COMMAND "${CARGO}" build --locked --release --package pl-lab
            --manifest-path "${CFS_RS_ROOT}/Cargo.toml"
            --target-dir "${CFS_RS_CARGO_DIR}"
    BYPRODUCTS "${CFS_RS_LIB}"
    COMMENT "cargo build pl-lab"
    VERBATIM
)

add_cfe_app(pl_lab fsw/src/pl_lab_glue.c)
add_dependencies(pl_lab pl_lab_rs_build)
target_link_libraries(pl_lab
    "${CFS_RS_LIB}"
)
target_link_options(pl_lab PRIVATE
    "-Wl,--undefined=PL_LAB_AppMain"
    "-Wl,--gc-sections"
    "-Wl,--version-script=${CMAKE_CURRENT_SOURCE_DIR}/pl_lab.sym"
)
set_property(TARGET pl_lab APPEND PROPERTY LINK_DEPENDS "${CFS_RS_LIB}" "${CMAKE_CURRENT_SOURCE_DIR}/pl_lab.sym")
