project(CMD_LAB C)

if (CMAKE_CROSSCOMPILING)
    message(FATAL_ERROR "cfs-rs: cross builds are not set up yet (no Rust target mapping)")
endif()

get_filename_component(CFS_RS_ROOT "${CMAKE_CURRENT_LIST_DIR}/../.." ABSOLUTE)

find_program(CARGO cargo REQUIRED)
set(CFS_RS_CARGO_DIR "${CMAKE_CURRENT_BINARY_DIR}/cargo")
set(CFS_RS_LIB "${CFS_RS_CARGO_DIR}/release/libcmd_lab.a")

add_custom_target(cmd_lab_rs_build
    COMMAND "${CARGO}" build --locked --release --package cmd-lab
            --manifest-path "${CFS_RS_ROOT}/Cargo.toml"
            --target-dir "${CFS_RS_CARGO_DIR}"
    BYPRODUCTS "${CFS_RS_LIB}"
    COMMENT "cargo build cmd-lab"
    VERBATIM
)

add_cfe_app(cmd_lab fsw/src/cmd_lab_glue.c)
add_dependencies(cmd_lab cmd_lab_rs_build)
target_link_libraries(cmd_lab
    "${CFS_RS_LIB}"
)
target_link_options(cmd_lab PRIVATE
    "-Wl,--undefined=CMD_LAB_AppMain"
    "-Wl,--gc-sections"
    "-Wl,--version-script=${CMAKE_CURRENT_SOURCE_DIR}/cmd_lab.sym"
)
set_property(TARGET cmd_lab APPEND PROPERTY LINK_DEPENDS "${CFS_RS_LIB}" "${CMAKE_CURRENT_SOURCE_DIR}/cmd_lab.sym")
