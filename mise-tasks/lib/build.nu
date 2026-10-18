#MISE hide=true
# Helpers shared by the nushell tasks. This module is not a task itself.

# Builds the plugin with cargo and returns the path of the executable.
#
# The path is taken from cargo's build messages, so it is right regardless of the configured
# target directory.
export def plugin [
    --release # Build with the release profile
    --target: string # Build for this target triple instead of the host
] {
    let args = [
        ...(if $release { [--release] } else { [] })
        ...(if $target != null { [--target $target] } else { [] })
    ]
    ^cargo build --locked ...$args --message-format=json-render-diagnostics
    | lines
    | each { from json }
    | where reason == compiler-artifact and executable? != null
    | where {|artifact| "bin" in $artifact.target.kind }
    | last
    | get executable
}
