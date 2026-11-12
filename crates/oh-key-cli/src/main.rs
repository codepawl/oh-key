//! `oh-key-cli`: try the typing engine from a terminal.

use oh_key_engine::{Config, Engine, InputMethod, KeyResult};
use std::process::ExitCode;

const USAGE: &str = "usage:
  oh-key-cli type [--method telex|vni|viqr|simple] <keystrokes>";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match arguments.first().map(String::as_str) {
        Some("type") => run_type(&arguments[1..]),
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
