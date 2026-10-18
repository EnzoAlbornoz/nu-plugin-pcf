# Builds the plugin and runs `tests/e2e.nu` in a fresh `nu` session that loads it with
# `--plugins`, leaving the user's plugin registry untouched.

use build.nu

const tests = path self ../tests/e2e.nu

def main [
    --release # Test the release build
] {
    let plugin = build plugin --release=$release
    ^$nu.current-exe --no-config-file --plugins $plugin $tests
}
