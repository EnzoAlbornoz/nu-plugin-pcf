use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// Reads a fixture, normalizing line endings in case Git converted them on checkout.
pub fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(fixture(name))
        .unwrap_or_else(|err| panic!("failed to read fixture {name}: {err}"))
        .replace("\r\n", "\n")
}

/// The `pkl` CLI, used to check the plugin against Pkl's own PCF renderer.
pub struct Pkl {
    program: OsString,
}

impl Pkl {
    /// Finds `pkl` (or the program in `$PKL`). Returns `None` when it is not installed, unless
    /// `$PKL_REQUIRED` is set, in which case a missing `pkl` fails the test.
    pub fn find() -> Option<Self> {
        let program = std::env::var_os("PKL").unwrap_or_else(|| "pkl".into());
        let runs = Command::new(&program)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success());
        if runs {
            Some(Self { program })
        } else if std::env::var_os("PKL_REQUIRED").is_some() {
            panic!("`pkl` is required (PKL_REQUIRED is set) but {program:?} could not be run");
        } else {
            eprintln!(
                "skipping: `pkl` is not installed, set $PKL to its location to run this test"
            );
            None
        }
    }

    /// Evaluates `module` and renders it with Pkl's PCF renderer.
    pub fn render_pcf(&self, module: &Path) -> String {
        let output = Command::new(&self.program)
            .args(["eval", "--format", "pcf"])
            .arg(module)
            .output()
            .expect("failed to run pkl");
        assert!(
            output.status.success(),
            "pkl failed to evaluate {}:\n{}",
            module.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("pkl output is UTF-8")
    }

    /// Writes `source` to a temporary module and renders it with Pkl's PCF renderer.
    pub fn render_pcf_source(&self, name: &str, source: &str) -> String {
        let module = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.pkl"));
        std::fs::write(&module, source).expect("failed to write temporary module");
        self.render_pcf(&module)
    }
}
