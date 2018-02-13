#![feature(box_syntax)]
#![feature(rustc_private)]

extern crate env_logger;
extern crate getopts;
#[macro_use]
extern crate log;
extern crate rustc;
extern crate rustc_driver;
extern crate rustc_errors;
extern crate rustc_trans_utils;
extern crate semverver;
extern crate syntax;

use semverver::semcheck::run_analysis;

use rustc::hir::def_id::*;
use rustc::middle::cstore::CrateStore;
use rustc::session::{config, Session};
use rustc::session::config::{Input, ErrorOutputType};

use rustc_driver::{driver, CompilerCalls, RustcDefaultCalls, Compilation};

use rustc_trans_utils::trans_crate::TransCrate;

use std::path::PathBuf;
use std::process::Command;

use syntax::ast;

/// After the typechecker has finished it's work, perform our checks.
///
/// To compare the two well-typed crates, first find the aptly named crates `new` and `old`,
/// find their root modules and then proceed to walk their module trees.
fn callback(state: &driver::CompileState, version: &str, verbose: bool) {
    let tcx = state.tcx.unwrap();

    let cnums = tcx
        .crates()
        .iter()
        .fold((None, None), |(o, n), crate_num| {
            let name = tcx.crate_name(*crate_num);
            if name == "old" {
                (Some(*crate_num), n)
            } else if name == "new" {
                (o, Some(*crate_num))
            } else {
                (o, n)
            }
        });

    let (old_def_id, new_def_id) = if let (Some(c0), Some(c1)) = cnums {
        (DefId {
             krate: c0,
             index: CRATE_DEF_INDEX,
         },
         DefId {
             krate: c1,
             index: CRATE_DEF_INDEX,
         })
    } else {
        tcx.sess.err("could not find crate `old` and/or `new`");
        return;
    };

    debug!("running semver analysis");
    let changes = run_analysis(tcx, old_def_id, new_def_id);

    changes.output(tcx.sess, version, verbose);
}

/// A wrapper to control compilation.
struct SemVerVerCompilerCalls {
    /// The wrapped compilation handle.
    default: RustcDefaultCalls,
    /// The version of the old crate.
    version: String,
}

impl SemVerVerCompilerCalls {
    /// Construct a new compilation wrapper, given a version string.
    pub fn new(version: String) -> SemVerVerCompilerCalls {
        SemVerVerCompilerCalls {
            default: RustcDefaultCalls,
            version: version,
        }
    }
}

impl<'a> CompilerCalls<'a> for SemVerVerCompilerCalls {
    fn early_callback(&mut self,
                      matches: &getopts::Matches,
                      sopts: &config::Options,
                      cfg: &ast::CrateConfig,
                      descriptions: &rustc_errors::registry::Registry,
                      output: ErrorOutputType)
                      -> Compilation {
        debug!("running rust-semverver early_callback");
        self.default
            .early_callback(matches, sopts, cfg, descriptions, output)
    }

    fn no_input(&mut self,
                matches: &getopts::Matches,
                sopts: &config::Options,
                cfg: &ast::CrateConfig,
                odir: &Option<PathBuf>,
                ofile: &Option<PathBuf>,
                descriptions: &rustc_errors::registry::Registry)
                -> Option<(Input, Option<PathBuf>)> {
        debug!("running rust-semverver no_input");
        self.default
            .no_input(matches, sopts, cfg, odir, ofile, descriptions)
    }

    fn late_callback(&mut self,
                     trans_crate: &TransCrate,
                     matches: &getopts::Matches,
                     sess: &Session,
                     cstore: &CrateStore,
                     input: &Input,
                     odir: &Option<PathBuf>,
                     ofile: &Option<PathBuf>)
                     -> Compilation {
        debug!("running rust-semverver late_callback");
        self.default
            .late_callback(trans_crate, matches, sess, cstore, input, odir, ofile)
    }

    fn build_controller(&mut self,
                        sess: &Session,
                        matches: &getopts::Matches)
                        -> driver::CompileController<'a> {
        let mut controller = self.default.build_controller(sess, matches);
        let old_callback =
            std::mem::replace(&mut controller.after_analysis.callback, box |_| {});
        let version = self.version.clone();

        let verbose = std::env::var("RUST_SEMVER_VERBOSE")
            .ok()
            .map_or(false, |s| s == "true");

        controller.after_analysis.callback = box move |state| {
            debug!("running rust-semverver after_analysis callback");
            callback(state, &version, verbose);
            debug!("running other after_analysis callback");
            old_callback(state);
        };
        controller.after_analysis.stop = Compilation::Stop;

        controller
    }
}

/// Main routine.
///
/// Find the sysroot before passing our args to the compiler driver, after registering our custom
/// compiler driver.
fn main() {
    if env_logger::try_init().is_err() {
        eprintln!("ERROR: could not initialize logger");
    }

    debug!("running rust-semverver compiler driver");

    let home = option_env!("RUSTUP_HOME");
    let toolchain = option_env!("RUSTUP_TOOLCHAIN");
    let sys_root = if let (Some(home), Some(toolchain)) = (home, toolchain) {
        format!("{}/toolchains/{}", home, toolchain)
    } else {
        option_env!("SYSROOT")
            .map(|s| s.to_owned())
            .or_else(|| {
                Command::new("rustc")
                    .args(&["--print", "sysroot"])
                    .output()
                    .ok()
                    .and_then(|out| String::from_utf8(out.stdout).ok())
                    .map(|s| s.trim().to_owned())
            })
            .expect("need to specify SYSROOT env var during compilation, or use rustup")
    };

    let result = rustc_driver::run(|| {
        let args: Vec<String> = if std::env::args().any(|s| s == "--sysroot") {
            std::env::args().collect()
        } else {
            std::env::args()
                .chain(Some("--sysroot".to_owned()))
                .chain(Some(sys_root))
                .collect()
        };

        let version = if let Ok(ver) = std::env::var("RUST_SEMVER_CRATE_VERSION") {
            ver
        } else {
            "no_version".to_owned()
        };

        let mut cc = SemVerVerCompilerCalls::new(version);
        rustc_driver::run_compiler(&args, &mut cc, None, None)
    });

    std::process::exit(result as i32);
}
