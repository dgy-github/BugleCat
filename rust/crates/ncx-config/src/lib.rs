//! ncx-config — nanocodex configuration loading.
//!
//! Rust port of `nanocodex/config.py`. Resolves a [`Config`] from multiple
//! sources with this priority order (highest wins):
//!
//! ```text
//! explicit overrides (CLI)
//!   > environment variables
//!     > profile bundle (NANOCODEX_PROFILE / [profiles.<name>])
//!       > ~/.nanocodex/config.toml
//!         > ~/.deepseek/config.toml
//!           > ~/.codex/config.toml
//!             > built-in defaults
//! ```

pub mod config;
pub mod loader;
pub mod writer;

pub use config::{
    derive_permission_mode, permission_mode_to_knobs, Config, ConfigError, HookConfig,
    McpServerConfig, VALID_APPROVAL_POLICIES, VALID_HOOK_EVENTS, VALID_PERMISSION_MODES,
    VALID_SANDBOX_MODES,
};
pub use loader::{
    list_profiles, list_profiles_at, load_config, load_config_with_paths, load_mcp_servers,
    load_mcp_servers_at, ConfigPaths, Overrides,
};
pub use writer::{dump_nanocodex_toml, write_nanocodex_config, WRITABLE_KEYS};
