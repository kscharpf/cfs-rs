project(HK_LAB C)

if (CMAKE_CROSSCOMPILING)
    message(FATAL_ERROR "cfs-rs: cross builds are not set up yet (no Rust target mapping)")
endif()

get_filename_component(CFS_RS_ROOT "${CMAKE_CURRENT_LIST_DIR}/../.." ABSOLUTE)

find_program(CARGO cargo REQUIRED)
set(CFS_RS_CARGO_DIR "${CMAKE_CURRENT_BINARY_DIR}/cargo")
set(CFS_RS_LIB "${CFS_RS_CARGO_DIR}/release/libhk_lab.a")

add_custom_target(hk_lab_rs_build
    COMMAND "${CARGO}" build --locked --release --package hk-lab
            --manifest-path "${CFS_RS_ROOT}/Cargo.toml"
            --target-dir "${CFS_RS_CARGO_DIR}"
    BYPRODUCTS "${CFS_RS_LIB}"
    COMMENT "cargo build hk-lab"
    VERBATIM
)

add_cfe_app(hk_lab fsw/src/hk_lab_glue.c)
add_dependencies(hk_lab hk_lab_rs_build)
target_link_libraries(hk_lab
    "${CFS_RS_LIB}"
)
target_link_options(hk_lab PRIVATE
    "-Wl,--undefined=HK_LAB_AppMain"
    "-Wl,--gc-sections"
    "-Wl,--version-script=${CMAKE_CURRENT_SOURCE_DIR}/hk_lab.sym"
)
set_property(TARGET hk_lab APPEND PROPERTY LINK_DEPENDS "${CFS_RS_LIB}" "${CMAKE_CURRENT_SOURCE_DIR}/hk_lab.sym")
