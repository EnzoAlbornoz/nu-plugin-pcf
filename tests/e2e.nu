# End-to-end tests against a real `nu`, run with `mise run e2e`.
#
# The `e2e` task builds the plugin and runs this file in a fresh `nu --plugins <path>` session,
# so the plugin is loaded without touching the user's plugin registry.

use std/assert

const root = path self ..
const fixtures = path self ./fixtures

def "test commands are registered" [] {
    let commands = help commands | where name in ["from pcf" "to pcf"]
    assert equal ($commands | get name | sort) ["from pcf" "to pcf"]
    assert equal ($commands | get category | uniq) [formats]

    let version = open ($root | path join Cargo.toml) | get package.version
    assert equal (plugin list | where name == pcf | get 0.version) $version
}

def "test open uses from pcf for .pcf files" [] {
    let config = open ($fixtures | path join all_types.pcf)
    assert equal $config.string Pkl
    assert equal $config.duration 5min
    assert equal $config.binaryDataSize 2.5GiB
    assert equal $config.listing.3 [nested]
    assert equal $config.intSeq 1..10
}

def "test save uses to pcf for .pcf files" [] {
    let dir = mktemp --directory
    let file = $dir | path join out.pcf
    let data = {name: nu, server: {ports: [80 443], timeout: 30sec}}
    $data | save $file
    assert equal (open --raw $file) "name = \"nu\"\nserver {\n  ports {\n    80\n    443\n  }\n  timeout = 30.s\n}\n"
    assert equal (open $file) $data
    rm --recursive $dir
}

def "test pkl output round trips" [] {
    # NaN never equals itself, so it is left out of the comparison.
    let original = open ($fixtures | path join all_types.pcf) | reject notANumber
    assert equal ($original | to pcf | from pcf) $original
}

def "test errors are reported" [] {
    let error = try { "a = 1\nb = " | from pcf; null } catch {|err| $err }
    assert equal $error.msg "Failed to parse PCF"

    let error = try { {f: {|| 1 }} | to pcf; null } catch {|err| $err }
    assert equal $error.msg "Failed to convert to PCF"
}

def check [name: string, test: closure] {
    print --no-newline $"($name) ... "
    do $test
    print ok
}

def main [] {
    check "commands are registered" { test commands are registered }
    check "open uses from pcf for .pcf files" { test open uses from pcf for .pcf files }
    check "save uses to pcf for .pcf files" { test save uses to pcf for .pcf files }
    check "pkl output round trips" { test pkl output round trips }
    check "errors are reported" { test errors are reported }
}
