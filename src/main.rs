use nu_plugin::{MsgPackSerializer, serve_plugin};
use nu_plugin_pcf::PcfPlugin;

fn main() {
    serve_plugin(&PcfPlugin, MsgPackSerializer);
}
