project(LINK_LAB C)

if (CMAKE_CROSSCOMPILING)
    message(FATAL_ERROR "cfs-rs: cross builds are not set up yet (no Rust target mapping)")
endif()

get_filename_component(CFS_RS_ROOT "${CMAKE_CURRENT_LIST_DIR}/../.." ABSOLUTE)

find_program(CARGO cargo REQUIRED)
set(CFS_RS_CARGO_DIR "${CMAKE_CURRENT_BINARY_DIR}/cargo")
set(CFS_RS_LIB "${CFS_RS_CARGO_DIR}/release/liblink_lab.a")

add_custom_target(link_lab_rs_build
    COMMAND "${CARGO}" build --locked --release --package link-lab
            --manifest-path "${CFS_RS_ROOT}/Cargo.toml"
            --target-dir "${CFS_RS_CARGO_DIR}"
    BYPRODUCTS "${CFS_RS_LIB}"
    COMMENT "cargo build link-lab"
    VERBATIM
)

add_cfe_app(link_lab fsw/src/link_lab_glue.c)
add_dependencies(link_lab link_lab_rs_build)
add_cfe_tables(link_lab fsw/tables/link_lab_cfg.c)
target_link_libraries(link_lab
    "${CFS_RS_LIB}"
)
target_link_options(link_lab PRIVATE
    "-Wl,--undefined=LINK_LAB_AppMain"
    "-Wl,--gc-sections"
    "-Wl,--version-script=${CMAKE_CURRENT_SOURCE_DIR}/link_lab.sym"
)
set_property(TARGET link_lab APPEND PROPERTY LINK_DEPENDS "${CFS_RS_LIB}" "${CMAKE_CURRENT_SOURCE_DIR}/link_lab.sym")
