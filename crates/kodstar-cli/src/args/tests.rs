// Purpose: 공개 명령 도움말 누락과 실행 예시의 실제 인자 계약 검증.
// Pattern: Contract Test.
// Usage: cargo test -p kodstar-cli --bin kodstar args::tests.
// Related: crates/kodstar-cli/src/args.rs, crates/kodstar-cli/src/invocation.rs.

use clap::{Command, error::ErrorKind};
use rstest::rstest;

fn visit(command: &Command, check: &mut impl FnMut(&Command)) {
    if command.is_hide_set() || command.get_name() == "help" {
        return;
    }
    check(command);
    for child in command.get_subcommands() {
        visit(child, check);
    }
}

#[test]
fn visible_commands_and_arguments_have_complete_help() {
    let mut command = super::command();
    command.build();
    visit(&command, &mut |command| {
        let name = command.get_name();
        for text in [command.get_about(), command.get_long_about()] {
            assert!(
                text.is_some_and(|s| !s.to_string().trim().is_empty()),
                "{name}"
            );
        }
        let examples = command
            .get_after_long_help()
            .expect("detailed examples")
            .to_string();
        assert!(
            examples
                .lines()
                .filter(|line| line.trim().starts_with("kodstar "))
                .count()
                >= 2,
            "{name}"
        );
        for arg in command.get_arguments().filter(|arg| !arg.is_hide_set()) {
            assert!(
                arg.get_help()
                    .is_some_and(|s| !s.to_string().trim().is_empty()),
                "{name}: missing description for {}",
                arg.get_id()
            );
        }
    });
}

#[test]
fn documented_examples_parse_without_executing_operations() {
    let mut command = super::command();
    command.build();
    visit(&command, &mut |command| {
        let examples = command
            .get_after_long_help()
            .expect("detailed examples")
            .to_string();
        for example in examples
            .lines()
            .filter_map(|line| line.trim().strip_prefix("kodstar "))
        {
            // Input redirection belongs to the shell; validate only the CLI arguments.
            let cli = example.split_once(" < ").map_or(example, |(cli, _)| cli);
            let arguments = std::iter::once("kodstar").chain(cli.split_whitespace());
            match super::command().try_get_matches_from(arguments) {
                Ok(matches) => {
                    crate::invocation::Invocation::parse(&matches).expect(example);
                }
                Err(error) => assert!(
                    matches!(
                        error.kind(),
                        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
                    ),
                    "invalid example: {example}\n{error}"
                ),
            }
        }
    });
}

#[rstest]
#[case::default(&["kodstar", "languages"], true)]
#[case::global_false(&["kodstar", "--verbose", "false", "languages"], false)]
#[case::command_false(&["kodstar", "languages", "--verbose", "false"], false)]
#[case::nested_false(&["kodstar", "history", "diff", "--verbose", "false"], false)]
#[case::explicit_true(&["kodstar", "--verbose", "true", "branch", "create", "work"], true)]
fn verbosity_defaults_to_true_and_propagates_from_global_and_nested_positions(
    #[case] arguments: &[&str],
    #[case] expected: bool,
) {
    let matches = super::command()
        .try_get_matches_from(arguments.iter().copied())
        .unwrap();
    assert_eq!(matches.get_one::<bool>("verbose"), Some(&expected));
    crate::invocation::Invocation::parse(&matches).unwrap();
}
