//! Command-line arguments.

use std::ffi::OsString;
use std::path::PathBuf;

const USAGE: &str = "\
Usage: retsurf [--game-mode] [PATH | --open ID]

  PATH          a game folder (opens its index.html) or an HTML file in one,
                served as http://<folder>.localhost/
  --open ID     open the bookmark bookmarks/ID.toml as it asks
  --game-mode   start in Game Mode
  -h, --help    print this help
  -V, --version print the version";

/// `sysexits.h`'s status for a bad command line.
const EX_USAGE: i32 = 64;

#[derive(Debug, Default, PartialEq)]
pub struct Args {
    /// Opened instead of the usual first tabs.
    pub path: Option<PathBuf>,
    /// A bookmark's id, opened instead of the usual first tabs.
    pub open: Option<String>,
    pub game_mode: bool,
}

#[derive(Debug, PartialEq)]
enum Parsed {
    Run(Args),
    Help,
    Version,
}

/// Exits after printing help, the version or an error.
pub fn from_env(version: &str) -> Args {
    match parse(std::env::args_os().skip(1)) {
        Ok(Parsed::Run(args)) => args,
        Ok(Parsed::Help) => {
            println!("{USAGE}");
            std::process::exit(0);
        }
        Ok(Parsed::Version) => {
            println!("retsurf {version}");
            std::process::exit(0);
        }
        Err(err) => fail(&err),
    }
}

pub fn fail(err: &str) -> ! {
    eprintln!("retsurf: {err}\n\n{USAGE}");
    std::process::exit(EX_USAGE);
}

fn parse(mut args: impl Iterator<Item = OsString>) -> Result<Parsed, String> {
    let mut out = Args::default();
    let mut only_paths = false;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--") if !only_paths => only_paths = true,
            Some("-h" | "--help") if !only_paths => return Ok(Parsed::Help),
            Some("-V" | "--version") if !only_paths => return Ok(Parsed::Version),
            Some("--game-mode") if !only_paths => out.game_mode = true,
            Some("--open") if !only_paths => {
                let id = args.next().ok_or("`--open` needs a bookmark id")?;
                let id = id.into_string().map_err(|_| "a bookmark id is text")?;
                out.open = Some(id);
            }
            Some(flag) if !only_paths && flag.starts_with('-') && flag != "-" => {
                return Err(format!("unknown option `{flag}`"));
            }
            _ if out.path.is_some() => return Err("only one PATH can be given".to_string()),
            _ => out.path = Some(PathBuf::from(arg)),
        }
    }
    if out.path.is_some() && out.open.is_some() {
        return Err("PATH and `--open` both name what to open".to_string());
    }
    Ok(Parsed::Run(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Result<Parsed, String> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn no_arguments_runs_as_before() {
        assert_eq!(run(&[]), Ok(Parsed::Run(Args::default())));
    }

    #[test]
    fn path_and_game_mode_in_any_order() {
        let want = Parsed::Run(Args {
            path: Some(PathBuf::from("games/digger")),
            game_mode: true,
            ..Args::default()
        });
        assert_eq!(run(&["--game-mode", "games/digger"]), Ok(want));
        assert!(matches!(
            run(&["games/digger", "--game-mode"]),
            Ok(Parsed::Run(Args {
                game_mode: true,
                ..
            }))
        ));
    }

    #[test]
    fn mistakes_are_errors() {
        assert!(run(&["--gamemode"]).is_err());
        assert!(run(&["a", "b"]).is_err());
        assert!(run(&["--open"]).is_err());
        assert!(run(&["--open", "ftl", "games/digger"]).is_err());
    }

    #[test]
    fn open_takes_a_bookmark_id() {
        assert_eq!(
            run(&["--game-mode", "--open", "ftl"]),
            Ok(Parsed::Run(Args {
                open: Some("ftl".to_string()),
                game_mode: true,
                ..Args::default()
            }))
        );
    }

    #[test]
    fn double_dash_ends_options() {
        assert_eq!(
            run(&["--", "--game-mode"]),
            Ok(Parsed::Run(Args {
                path: Some(PathBuf::from("--game-mode")),
                ..Args::default()
            }))
        );
    }

    #[test]
    fn help_and_version() {
        assert_eq!(run(&["--help"]), Ok(Parsed::Help));
        assert_eq!(run(&["-V"]), Ok(Parsed::Version));
    }
}
