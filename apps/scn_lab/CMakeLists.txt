project(SCN_LAB C)

if (CMAKE_CROSSCOMPILING)
    message(FATAL_ERROR "cfs-rs: cross builds are not set up yet (no Rust target mapping)")
endif()

get_filename_component(CFS_RS_ROOT "${CMAKE_CURRENT_LIST_DIR}/../.." ABSOLUTE)

find_program(CARGO cargo REQUIRED)
set(CFS_RS_CARGO_DIR "${CMAKE_CURRENT_BINARY_DIR}/cargo")
set(CFS_RS_LIB "${CFS_RS_CARGO_DIR}/release/libscn_lab.a")

add_custom_target(scn_lab_rs_build
    COMMAND "${CARGO}" build --locked --release --package scn-lab
            --manifest-path "${CFS_RS_ROOT}/Cargo.toml"
            --target-dir "${CFS_RS_CARGO_DIR}"
    BYPRODUCTS "${CFS_RS_LIB}"
    COMMENT "cargo build scn-lab"
    VERBATIM
)

add_cfe_app(scn_lab fsw/src/scn_lab_glue.c)
add_dependencies(scn_lab scn_lab_rs_build)
target_link_libraries(scn_lab
    -Wl,--whole-archive
    "${CFS_RS_LIB}"
    -Wl,--no-whole-archive
)
target_link_options(scn_lab PRIVATE
    "-Wl,--version-script=${CMAKE_CURRENT_SOURCE_DIR}/scn_lab.sym"
)
set_property(TARGET scn_lab APPEND PROPERTY LINK_DEPENDS "${CFS_RS_LIB}" "${CMAKE_CURRENT_SOURCE_DIR}/scn_lab.sym")
