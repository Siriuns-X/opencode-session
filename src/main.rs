use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("ls") => list_sessions(),
        Some("rm") => remove_session(&args[1..]),
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("usage:");
    eprintln!("\tocs ls");
    eprintln!("\tocs rm <N>");
    eprintln!("\tocs rm --id <session_id>");
    ExitCode::FAILURE
}

fn list_sessions() -> ExitCode {
    ExitCode::SUCCESS
}

fn remove_session(_rest: &[String]) -> ExitCode {
    ExitCode::SUCCESS
}
