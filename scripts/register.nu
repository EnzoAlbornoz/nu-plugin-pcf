# Installs the plugin with `cargo install` and registers it with nu.

const root = path self ..

def main [] {
    ^cargo install --locked --path $root
    let cargo_home = $env.CARGO_HOME? | default ($nu.home-dir | path join .cargo)
    let executable = if $nu.os-info.name == windows { "nu_plugin_pcf.exe" } else { "nu_plugin_pcf" }
    let plugin = $cargo_home | path join bin $executable
    plugin add $plugin
    print $"Registered ($plugin). Run `plugin use pcf` or start a new nu session to use it."
}
