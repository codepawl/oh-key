//! `oh-key`: try the engine from a terminal and register the Windows input method.

use oh_key_engine::{Config, Engine, InputMethod, KeyResult};
use std::path::PathBuf;
use std::process::{Command, ExitCode};

const USAGE: &str = "usage:
  oh-key type [--method telex|vni|viqr|simple] <keystrokes>
  oh-key register [path\\to\\ohkey_tip.dll]
  oh-key unregister [path\\to\\ohkey_tip.dll]";

const DLL_FILE_NAME: &str = "ohkey_tip.dll";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match arguments.first().map(String::as_str) {
        Some("type") => run_type(&arguments[1..]),
        Some("register") => run_regsvr32(&arguments[1..], false),
        Some("unregister") => run_regsvr32(&arguments[1..], true),
        _ => Err(USAGE.to_owned()),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run_type(arguments: &[String]) -> Result<(), String> {
    let mut input_method = InputMethod::Telex;
    let mut keystrokes = Vec::new();
    let mut remaining = arguments.iter();
    while let Some(argument) = remaining.next() {
        if argument == "--method" {
            let name = remaining.next().ok_or("--method needs a value")?;
            input_method = parse_method(name)?;
        } else {
            keystrokes.push(argument.as_str());
        }
    }
    if keystrokes.is_empty() {
        return Err(USAGE.to_owned());
    }
    println!("{}", type_keystrokes(input_method, &keystrokes.join(" ")));
    Ok(())
}

fn parse_method(name: &str) -> Result<InputMethod, String> {
    match name.to_ascii_lowercase().as_str() {
        "telex" => Ok(InputMethod::Telex),
        "vni" => Ok(InputMethod::Vni),
        "viqr" => Ok(InputMethod::Viqr),
        "simple" | "simple-telex" => Ok(InputMethod::SimpleTelex),
        other => Err(format!("unknown input method: {other}")),
    }
}

/// Feeds every character to the engine and returns the text it would type.
fn type_keystrokes(input_method: InputMethod, keystrokes: &str) -> String {
    let mut engine = Engine::new(Config {
        input_method,
        ..Config::default()
    });
    let mut output = String::new();
    for key in keystrokes.chars() {
        match engine.process_key(key) {
            KeyResult::Composing => {}
            KeyResult::Commit(text) => output.push_str(&text),
            KeyResult::PassThrough => output.push(key),
        }
    }
    output.push_str(&engine.commit());
    output
}

fn run_regsvr32(arguments: &[String], unregister: bool) -> Result<(), String> {
    let dll_path = locate_dll(arguments.first())?;
    let mut command = Command::new("regsvr32");
    if unregister {
        command.arg("/u");
    }
    let status = command
        .arg("/s")
        .arg(&dll_path)
        .status()
        .map_err(|error| format!("could not start regsvr32: {error}"))?;
    if status.success() {
        println!(
            "{} {}",
            if unregister {
                "unregistered"
            } else {
                "registered"
            },
            dll_path.display()
        );
        return Ok(());
    }
    Err(format!(
        "regsvr32 failed with {status}; registering writes to HKLM, so run this from an elevated terminal"
    ))
}

/// The DLL next to this program unless a path is given.
fn locate_dll(explicit_path: Option<&String>) -> Result<PathBuf, String> {
    let path = match explicit_path {
        Some(path) => PathBuf::from(path),
        None => {
            let executable = std::env::current_exe().map_err(|error| error.to_string())?;
            let directory = executable
                .parent()
                .ok_or("cannot find the program folder")?;
            directory.join(DLL_FILE_NAME)
        }
    };
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("{} not found", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_a_telex_sentence() {
        assert_eq!(
            type_keystrokes(InputMethod::Telex, "Vieetj Nam"),
            "Việt Nam"
        );
    }

    #[test]
    fn types_vni() {
        assert_eq!(type_keystrokes(InputMethod::Vni, "to6i"), "tôi");
    }
}
