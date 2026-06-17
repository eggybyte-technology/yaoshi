use std::env;
use std::process::ExitCode;

use yaoshi_common::{ExitKind, VERSION, YaoshiError, YaoshiResult};

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::from(ExitKind::Ok.code() as u8),
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(err.kind().code() as u8)
        }
    }
}

fn run(args: Vec<String>) -> YaoshiResult<()> {
    match args.as_slice() {
        [] => yaoshi_build::run_from_current_dir(),
        [arg] if arg == "--version" => {
            println!("yaoshi {VERSION}");
            Ok(())
        }
        _ => Err(YaoshiError::usage(
            "invalid arguments; expected no arguments or --version",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_subcommands() {
        let err = run(vec!["img".to_string()]).unwrap_err();
        assert_eq!(err.kind(), ExitKind::Usage);
    }

    #[test]
    fn rejects_help() {
        let err = run(vec!["--help".to_string()]).unwrap_err();
        assert_eq!(err.kind(), ExitKind::Usage);
    }
}
