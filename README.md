# nu_plugin_pcf

[![CI](https://github.com/EnzoAlbornoz/nu-plugin-pcf/actions/workflows/ci.yml/badge.svg)](https://github.com/EnzoAlbornoz/nu-plugin-pcf/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/EnzoAlbornoz/nu-plugin-pcf)](https://github.com/EnzoAlbornoz/nu-plugin-pcf/releases)

A [Nushell](https://www.nushell.sh) plugin that adds `from pcf` and `to pcf`, so Nushell can read
and write PCF, the Pkl Configuration Format.

PCF is the static subset of [Pkl](https://pkl-lang.org): plain properties, objects, listings,
mappings and literals, with no expressions. It is what `pkl eval -f pcf` produces and what Pkl
reads back as a regular module.

```nushell
> open config.pcf
╭────────┬───────────────────────────╮
│ name   │ api                       │
│        │ ╭─────────┬─────────────╮ │
│ server │ │ host    │ example.com │ │
│        │ │ port    │ 8080        │ │
│        │ │ timeout │ 30sec       │ │
│        │ ╰─────────┴─────────────╯ │
╰────────┴───────────────────────────╯

> {name: api, server: {hosts: [a b], timeout: 30sec, maxBody: 5MB}} | to pcf
name = "api"
server {
  hosts {
    "a"
    "b"
  }
  timeout = 30.s
  maxBody = 5.mb
}
```

## Installation

The plugin is built against Nushell **0.116**. Nushell only loads plugins built for its own minor
version, so use a release that matches your `version | get version`.

### Prebuilt binaries

Every [release](https://github.com/EnzoAlbornoz/nu-plugin-pcf/releases) has archives for:

| Platform              | Archive                                         |
|-----------------------|-------------------------------------------------|
| macOS (Apple silicon) | `nu_plugin_pcf-<version>-aarch64-apple-darwin.tar.gz` |
| Linux (x86-64)        | `nu_plugin_pcf-<version>-x86_64-unknown-linux-musl.tar.gz` |
| Windows (x86-64)      | `nu_plugin_pcf-<version>-x86_64-pc-windows-msvc.zip` |

The Linux binary is statically linked and runs on any distribution. The release also has a
`SHA256SUMS` file, and every archive has a
[build provenance attestation](https://docs.github.com/actions/security-for-github-actions/using-artifact-attestations)
that you can check with `gh attestation verify <archive> --repo EnzoAlbornoz/nu-plugin-pcf`.

Extract the archive somewhere permanent, then register the plugin:

```nushell
plugin add ~/.local/bin/nu_plugin_pcf   # nu_plugin_pcf.exe on Windows
plugin use pcf
```

### From source

With a Rust toolchain installed:

```nushell
cargo install --locked --git https://github.com/EnzoAlbornoz/nu-plugin-pcf
plugin add ~/.cargo/bin/nu_plugin_pcf
plugin use pcf
```

## Usage

`open` and `save` pick the commands from the `.pcf` extension:

```nushell
open config.pcf | get server.port
$config | save config.pcf
```

Pkl modules that use expressions, `amends` or imports have to be evaluated first:

```nushell
pkl eval -f pcf config.pkl | from pcf
```

### `from pcf`

Parses PCF text (or UTF-8 binary data) into a record. Errors point at the line and column of the
problem:

```text
Error:   × Failed to parse PCF
   ╭─[entry #1:1:1]
 1 │ "port = 80 80" | from pcf
   · ───────┬──────
   ·        ╰── only properties are allowed at the top level of a PCF document at line 1, column 11
   ╰────
  help: 1 | port = 80 80
          |           ^^
```

### `to pcf`

Renders a record as PCF, with the same layout as `pkl eval -f pcf`.

| Flag                           | Description                                         |
|--------------------------------|-----------------------------------------------------|
| `--indent (-i) <int>`          | Spaces per indentation level (default: 2)           |
| `--tabs (-t) <int>`            | Tabs per indentation level                          |
| `--omit-null-properties (-n)`  | Leave out fields whose value is `null`              |

## Type mapping

### PCF to Nushell

| PCF                                             | Nushell                         |
|-------------------------------------------------|---------------------------------|
| `String`, `Int`, `Float`, `Boolean`, `null`     | string, int, float, bool, nothing |
| object with properties, `Mapping`, `Map(...)`   | record                          |
| object with elements, `Listing`, `List(...)`, `Set(...)` | list                   |
| `Duration` (`5.min`)                            | duration                        |
| `DataSize` (`5.mb`, `2.gib`)                    | filesize                        |
| `IntSeq(1, 10)`, `IntSeq(1, 10).step(2)`        | range                           |
| `Bytes(1, 2, 3)`                                | binary                          |
| `Pair(a, b)`                                    | record `{first: a, second: b}`  |
| `Regex("...")`                                  | string with the pattern         |

Keys of mappings that are not strings, such as `[1] = "a"`, become their PCF representation (`"1"`).

### Nushell to PCF

| Nushell              | PCF                                                         |
|----------------------|-------------------------------------------------------------|
| record               | object (`name { ... }`)                                     |
| list                 | listing (`name { ... }`), `new Listing {}` when empty       |
| string               | string, multi-line (`"""`) when it contains line breaks     |
| int, float, bool, nothing | `Int`, `Float`, `Boolean`, `null`                      |
| duration             | `Duration` in the largest exact unit (`90.min`)             |
| filesize             | `DataSize` in the largest exact unit (`2.mib`)              |
| range                | `IntSeq`                                                    |
| binary               | `Bytes(...)`                                                |
| date                 | RFC 3339 string                                             |

Field names that are not valid Pkl identifiers are quoted with backticks (`` `my-key` ``). Names
that cannot be quoted (containing a backtick or a line break) are written as entries
(`["my`key"] = ...`) inside nested records.

### Limitations

- Only static PCF is supported. References, string interpolation, `amends`, imports, classes and
  other declarations are rejected with an error that suggests evaluating the module with `pkl`.
- Objects that mix elements with properties or entries have no Nushell equivalent and are
  rejected, just like Pkl's own JSON renderer does.
- Converting back to PCF loses some distinctions: `List`, `Set` and `Listing` all come back as
  listings, `Mapping` entries come back as properties, and `Pair` and `Regex` become a record and a
  string.
- Durations are stored in whole nanoseconds and data sizes in whole bytes, so fractional values
  such as `1.5.b` are rounded.
- Pkl itself renders an empty `Listing` as `{}`, which reads back as an empty record.

## Development

The toolchain (Rust, Nushell, Pkl, git-cliff and the linters) is managed with
[mise](https://mise.jdx.dev):

```sh
mise install        # install the pinned tools
mise run test       # unit, integration and Pkl interop tests
mise run e2e        # end-to-end tests in a real nu session
mise run lint       # rustfmt, clippy, workflow and commit message checks
mise run register   # install the plugin and register it with your nu
mise tasks          # list every task
```

Simple tasks are defined in `mise.toml`. Tasks with more logic (`e2e`, `package`, `register` and
`release`) are nushell scripts in [`mise-tasks/`](mise-tasks), which also run on Windows.

The interop tests compare the plugin with Pkl's own PCF renderer. They are skipped when `pkl` is not
installed, except under `mise run test` and in CI, where `PKL_REQUIRED=1` makes them mandatory.
`tests/fixtures/all_types.pcf` is Pkl's rendering of `all_types.pkl`; regenerate it with
`mise run fixtures`.

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org), which is checked
on pull requests and used to generate the changelog.

### Releasing

```sh
mise run release 0.2.0   # bump Cargo.toml, regenerate CHANGELOG.md, commit and tag v0.2.0
git push --follow-tags
```

The tag starts the [release workflow](.github/workflows/release.yml), which builds the archives,
attests them and publishes a GitHub release with notes generated from the commits. Run the
workflow manually to build the archives without publishing anything.

## License

[MIT](LICENSE)
