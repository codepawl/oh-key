//! Key handling for the oh-key IBus engine. The C glue in `linux/ibus/` owns the IBus objects and
//! calls this crate for every key; everything that decides what is typed lives here, so it is
//! tested on every system without IBus.

mod c_api;
mod config;
mod keys;
mod typing;

pub use config::{ConfigFiles, config_folder};
pub use keys::{KeyKind, classify_keysym};
pub use typing::{KeyboardEngine, PreeditChange, Reaction};
