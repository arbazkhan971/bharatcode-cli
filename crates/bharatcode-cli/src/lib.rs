#[cfg(not(any(feature = "rustls-tls", feature = "native-tls")))]
compile_error!("At least one of `rustls-tls` or `native-tls` features must be enabled");

#[cfg(all(feature = "rustls-tls", feature = "native-tls"))]
compile_error!("Features `rustls-tls` and `native-tls` are mutually exclusive");

pub mod a11y;
pub mod cli;
pub mod commands;
// BharatCode v95: offline static docs-site generator (`bharatcode docgen-site`).
// `docsite::generate_site` walks the live `Cli::command()` clap tree plus the
// embedded built-in skills and emits a self-contained Markdown docs set, driven
// by the canonical `bharatcode_core::doc_manifest::pages()` single source of truth.
pub mod docsite;
pub mod help_index;
pub mod i18n;
pub mod keybindings;
pub mod logging;
pub mod notify;
pub mod project_tracker;
pub mod recipes;
pub mod scenario_tests;
pub mod session;
pub mod signal;
pub mod subagent_settings;
pub mod theme;

// Re-export commonly used types
pub use cli::Cli;
pub use session::CliSession;
