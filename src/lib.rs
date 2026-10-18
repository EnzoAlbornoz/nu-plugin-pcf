//! A [Nushell](https://www.nushell.sh) plugin that adds `from pcf` and `to pcf`, converting
//! between Nushell values and PCF, the static subset of [Pkl](https://pkl-lang.org) that
//! `pkl eval -f pcf` produces.

mod commands;
pub mod pcf;

use nu_plugin::{Plugin, PluginCommand};

pub use commands::{FromPcf, ToPcf};

/// The plugin entry point, serving the `from pcf` and `to pcf` commands.
pub struct PcfPlugin;

impl Plugin for PcfPlugin {
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").into()
    }

    fn commands(&self) -> Vec<Box<dyn PluginCommand<Plugin = Self>>> {
        vec![Box::new(FromPcf), Box::new(ToPcf)]
    }
}
