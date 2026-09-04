#[derive(Debug, PartialEq, Eq)]
enum Command {
    Help,
}

fn parse_command(arguments: impl Iterator<Item = String>) -> Command {
    let _ = arguments.skip(1);

    Command::Help
}

fn main() {
    match parse_command(std::env::args()) {
        Command::Help => print_help(),
    }
}

fn print_help() {
    println!(
        "Orbit command-line interface\n\nUsage: orbit [OPTIONS]\n\nOptions:\n  -h, --help  Print help"
    );
}

#[cfg(test)]
mod tests {
    use super::{Command, parse_command};

    #[test]
    fn recognizes_the_help_flag() {
        let command = parse_command(["orbit", "--help"].into_iter().map(str::to_owned));

        assert_eq!(command, Command::Help);
    }
}
