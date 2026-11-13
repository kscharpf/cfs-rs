/*
 * Minimal C compilation unit for CMake add_cfe_app.
 * Application logic and symbols live in the Rust static library.
 */

/*
 * Rust precompiled core references rust_eh_personality for unwinding.
 * Since this crate compiles with panic = "abort", unwinding never occurs.
 * A dummy definition satisfies dynamic linking on dlopen.
 */
void rust_eh_personality(void)
{
}
