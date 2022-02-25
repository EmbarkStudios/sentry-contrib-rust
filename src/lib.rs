// BEGIN - Embark standard lints v5 for Rust 1.55+
// do not change or add/remove here, but one can add exceptions after this section
// for more info see: <https://github.com/EmbarkStudios/rust-ecosystem/issues/59>
#![deny(unsafe_code)]
#![warn(
    clippy::all,
    clippy::await_holding_lock,
    clippy::char_lit_as_u8,
    clippy::checked_conversions,
    clippy::dbg_macro,
    clippy::debug_assert_with_mut_call,
    clippy::doc_markdown,
    clippy::empty_enum,
    clippy::enum_glob_use,
    clippy::exit,
    clippy::expl_impl_clone_on_copy,
    clippy::explicit_deref_methods,
    clippy::explicit_into_iter_loop,
    clippy::fallible_impl_from,
    clippy::filter_map_next,
    clippy::flat_map_option,
    clippy::float_cmp_const,
    clippy::fn_params_excessive_bools,
    clippy::from_iter_instead_of_collect,
    clippy::if_let_mutex,
    clippy::implicit_clone,
    clippy::imprecise_flops,
    clippy::inefficient_to_string,
    clippy::invalid_upcast_comparisons,
    clippy::large_digit_groups,
    clippy::large_stack_arrays,
    clippy::large_types_passed_by_value,
    clippy::let_unit_value,
    clippy::linkedlist,
    clippy::lossy_float_literal,
    clippy::macro_use_imports,
    clippy::manual_ok_or,
    clippy::map_err_ignore,
    clippy::map_flatten,
    clippy::map_unwrap_or,
    clippy::match_on_vec_items,
    clippy::match_same_arms,
    clippy::match_wild_err_arm,
    clippy::match_wildcard_for_single_variants,
    clippy::mem_forget,
    clippy::mismatched_target_os,
    clippy::missing_enforced_import_renames,
    clippy::mut_mut,
    clippy::mutex_integer,
    clippy::needless_borrow,
    clippy::needless_continue,
    clippy::needless_for_each,
    clippy::option_option,
    clippy::path_buf_push_overwrite,
    clippy::ptr_as_ptr,
    clippy::rc_mutex,
    clippy::ref_option_ref,
    clippy::rest_pat_in_fully_bound_structs,
    clippy::same_functions_in_if_condition,
    clippy::semicolon_if_nothing_returned,
    clippy::single_match_else,
    clippy::string_add_assign,
    clippy::string_add,
    clippy::string_lit_as_bytes,
    clippy::string_to_string,
    clippy::todo,
    clippy::trait_duplication_in_bounds,
    clippy::unimplemented,
    clippy::unnested_or_patterns,
    clippy::unused_self,
    clippy::useless_transmute,
    clippy::verbose_file_reads,
    clippy::zero_sized_map_values,
    future_incompatible,
    nonstandard_style,
    rust_2018_idioms
)]
// END - Embark standard lints v0.5 for Rust 1.55+

//! Capture and report minidumps to [Sentry](https://sentry.io/about/)
//!
//! 1. Create a [`BreakpadTransportFactory`] for [`ClientOptions::transport`](https://docs.rs/sentry-core/0.23.0/sentry_core/struct.ClientOptions.html#structfield.transport)
//! , providing it with the [`TransportFactory`](https://docs.rs/sentry-core/0.23.0/sentry_core/trait.TransportFactory.html)
//! you were previously using.
//! 2. Initialize a Sentry [`Hub`](https://docs.rs/sentry-core/0.23.0/sentry_core/struct.Hub.html).
//! 3. Create the [`BreakpadIntegration`] which will attach a crash handler and
//! send any previous crashes that are in the crash directoy specified.

macro_rules! debug_print {
    ($($arg:tt)*) => {
        #[cfg(feature = "debug-logs")]
        {
            eprintln!("[bp] {}", format_args!($($arg)*));
        }
        #[cfg(not(feature = "debug-logs"))]
        {
            let _ = format_args!($($arg)*);
        }
    }
}

mod breakpad_integration;
mod error;
mod shared;
mod transport;

pub use breakpad_integration::{BreakpadIntegration, InstallOptions};
pub use error::Error;
pub use transport::{BreakpadTransportFactory, CrashSendStyle};
