use std::path::{Path, PathBuf};

use agent_client_protocol::schema::v1::{ToolCallLocation, ToolKind};
use codex_protocol::parse_command::ParsedCommand;
use codex_shell_command::parse_command::parse_command;

pub(crate) struct ParseCommandToolCall {
    pub(crate) title: String,
    pub(crate) file_extension: Option<String>,
    pub(crate) terminal_output: bool,
    pub(crate) locations: Vec<ToolCallLocation>,
    pub(crate) kind: ToolKind,
}

pub(crate) fn parse_command_tool_call(
    parsed_cmd: Vec<ParsedCommand>,
    cwd: &Path,
) -> ParseCommandToolCall {
    let mut titles = Vec::new();
    let mut locations = Vec::new();
    let mut file_extension = None;
    let mut terminal_output = false;
    let mut kind = ToolKind::Execute;

    for cmd in parsed_cmd {
        let mut cmd_path = None;
        match cmd {
            ParsedCommand::Read { cmd: _, name, path } => {
                titles.push(format!("Read {name}"));
                file_extension = path_extension(&path);
                cmd_path = Some(path);
                kind = ToolKind::Read;
            }
            ParsedCommand::ListFiles { cmd: _, path } => {
                let dir = if let Some(path) = path.as_ref() {
                    &cwd.join(path)
                } else {
                    cwd
                };
                titles.push(format!("List {}", dir.display()));
                cmd_path = path.map(PathBuf::from);
                terminal_output = true;
                kind = ToolKind::Search;
            }
            ParsedCommand::Search { cmd, query, path } => {
                titles.push(match (query, path.as_ref()) {
                    (Some(query), Some(path)) => format!("Search {query} in {path}"),
                    (Some(query), None) => format!("Search {query}"),
                    _ => format!("Search {cmd}"),
                });
                kind = ToolKind::Search;
            }
            ParsedCommand::Unknown { cmd } => {
                append_tool_call(
                    &mut titles,
                    &mut file_extension,
                    &mut terminal_output,
                    &mut locations,
                    &mut kind,
                    parse_unknown_command_tool_call(cmd, cwd),
                );
            }
        }

        if let Some(path) = cmd_path {
            locations.push(tool_call_location(cwd, path));
        }
    }

    ParseCommandToolCall {
        title: titles.join(", "),
        file_extension,
        terminal_output,
        locations,
        kind,
    }
}

fn append_tool_call(
    titles: &mut Vec<String>,
    file_extension: &mut Option<String>,
    terminal_output: &mut bool,
    locations: &mut Vec<ToolCallLocation>,
    kind: &mut ToolKind,
    parsed: ParseCommandToolCall,
) {
    if !parsed.title.is_empty() {
        titles.push(parsed.title);
    }
    if parsed.file_extension.is_some() {
        *file_extension = parsed.file_extension;
    }
    *terminal_output |= parsed.terminal_output;
    locations.extend(parsed.locations);
    *kind = parsed.kind;
}

fn parse_unknown_command_tool_call(cmd: String, cwd: &Path) -> ParseCommandToolCall {
    if let Some(parsed_cmd) = reparse_unknown_command(&cmd) {
        return parse_command_tool_call(parsed_cmd, cwd);
    }

    if is_git_diff_command(&cmd) {
        return command_tool_call(
            cmd,
            Some("diff".to_string()),
            false,
            Vec::new(),
            ToolKind::Read,
        );
    }

    if is_search_command(&cmd) {
        return command_tool_call(
            format!("Search {cmd}"),
            None,
            false,
            Vec::new(),
            ToolKind::Search,
        );
    }

    if let Some(path) = file_read_path_from_unknown_command(&cmd, cwd) {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .map_or_else(|| path.display().to_string(), ToOwned::to_owned);
        return command_tool_call(
            format!("Read {name}"),
            path_extension(&path),
            false,
            vec![tool_call_location(cwd, path)],
            ToolKind::Read,
        );
    }

    command_tool_call(cmd, None, true, Vec::new(), ToolKind::Execute)
}

fn command_tool_call(
    title: String,
    file_extension: Option<String>,
    terminal_output: bool,
    locations: Vec<ToolCallLocation>,
    kind: ToolKind,
) -> ParseCommandToolCall {
    ParseCommandToolCall {
        title,
        file_extension,
        terminal_output,
        locations,
        kind,
    }
}

fn tool_call_location(cwd: &Path, path: PathBuf) -> ToolCallLocation {
    ToolCallLocation::new(if path.is_relative() {
        cwd.join(path)
    } else {
        path
    })
}

fn reparse_unknown_command(cmd: &str) -> Option<Vec<ParsedCommand>> {
    let argv = command_argv(cmd);
    let argv = strip_known_command_wrappers(&argv);
    if argv.is_empty() {
        return None;
    }

    // Codex reports shell scripts as Unknown once any stage is ambiguous. Reuse
    // its bash parser before applying ACP-only fallbacks so display categories
    // stay aligned with the main harness for read/search/list pipelines.
    if argv_has_shell_operator(argv) {
        let shell_command = vec!["bash".to_string(), "-lc".to_string(), cmd.to_string()];
        return parse_meaningful_command(&shell_command);
    }

    parse_meaningful_command(argv)
}

fn parse_meaningful_command(argv: &[String]) -> Option<Vec<ParsedCommand>> {
    let parsed = parse_command(argv);
    if parsed
        .iter()
        .all(|cmd| matches!(cmd, ParsedCommand::Unknown { .. }))
    {
        None
    } else {
        Some(parsed)
    }
}

fn file_read_path_from_unknown_command(cmd: &str, cwd: &Path) -> Option<PathBuf> {
    let argv = command_argv(cmd);
    let argv = strip_known_command_wrappers(&argv);
    if argv_has_shell_operator(argv) {
        return None;
    }

    let program = argv
        .first()
        .and_then(|program| Path::new(program).file_name())
        .and_then(|program| program.to_str())?;

    if !is_file_read_program(program) {
        return None;
    }

    file_read_path_from_args(program, &argv[1..], cwd)
}

fn file_read_path_from_args(program: &str, args: &[String], cwd: &Path) -> Option<PathBuf> {
    if args_imply_file_mutation(program, args) {
        return None;
    }

    let candidates = if program == "sed" {
        sed_input_file_candidates(args)
    } else {
        args.iter()
            .filter(|arg| arg.as_str() != "--" && !arg.starts_with('-'))
            .cloned()
            .collect()
    };

    candidates
        .into_iter()
        .rev()
        .map(PathBuf::from)
        .find(|path| path_has_extension(path) && command_read_path_exists(cwd, path))
}

fn sed_input_file_candidates(args: &[String]) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut saw_inline_script = false;
    let mut index = 0;

    while let Some(arg) = args.get(index) {
        match arg.as_str() {
            "--" => {
                candidates.extend(args[index + 1..].iter().cloned());
                break;
            }
            "-e" | "--expression" | "-f" | "--file" => {
                saw_inline_script = true;
                index += 2;
            }
            "-i" | "--in-place" => return Vec::new(),
            "-n" | "--quiet" | "--silent" | "-E" | "-r" | "-u" | "--unbuffered" | "-s"
            | "--separate" | "--posix" | "-z" | "--null-data" => {
                index += 1;
            }
            _ if arg.starts_with("--expression=") || arg.starts_with("--file=") => {
                saw_inline_script = true;
                index += 1;
            }
            _ if arg.starts_with("--in-place=") => return Vec::new(),
            _ if arg.starts_with("--") => {
                index += 1;
            }
            _ if arg.starts_with('-') && arg.len() > 2 => {
                if sed_short_option_has_in_place(arg) {
                    return Vec::new();
                }
                if arg.contains('e') || arg.contains('f') {
                    saw_inline_script = true;
                }
                index += 1;
            }
            _ if !saw_inline_script => {
                saw_inline_script = true;
                index += 1;
            }
            _ => {
                candidates.push(arg.clone());
                index += 1;
            }
        }
    }

    candidates
}

fn sed_short_option_has_in_place(arg: &str) -> bool {
    let Some(options) = arg.strip_prefix('-').filter(|arg| !arg.starts_with('-')) else {
        return false;
    };

    for option in options.chars() {
        match option {
            'e' | 'f' => return false,
            'i' => return true,
            _ => {}
        }
    }

    false
}

fn path_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| !ext.is_empty())
        .map(str::to_ascii_lowercase)
}

fn path_has_extension(path: &Path) -> bool {
    path_extension(path).is_some()
}

fn command_read_path_exists(cwd: &Path, path: &Path) -> bool {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    path.is_file()
}

fn strip_known_command_wrappers(argv: &[String]) -> &[String] {
    let mut argv = argv;
    loop {
        let Some(program) = argv
            .first()
            .and_then(|program| Path::new(program).file_name())
            .and_then(|program| program.to_str())
        else {
            return argv;
        };

        match program {
            "command" => argv = &argv[1..],
            _ if program.contains('=') && !program.contains('/') => argv = &argv[1..],
            _ => return argv,
        }
    }
}

fn is_file_read_program(program: &str) -> bool {
    matches!(
        program,
        "bat" | "batcat" | "cat" | "head" | "jq" | "less" | "more" | "nl" | "sed" | "tail" | "yq"
    )
}

fn is_git_diff_command(cmd: &str) -> bool {
    let argv = command_argv(cmd);
    let argv = strip_known_command_wrappers(&argv);
    if argv_has_shell_operator(argv) {
        return false;
    }

    let Some(program) = argv
        .first()
        .and_then(|program| Path::new(program).file_name())
        .and_then(|program| program.to_str())
    else {
        return false;
    };

    if program != "git" {
        return false;
    }

    let Some((subcommand_index, subcommand)) = git_subcommand(argv) else {
        return false;
    };

    subcommand == "diff" && !git_diff_uses_summary_output(&argv[subcommand_index + 1..])
}

fn git_subcommand(argv: &[String]) -> Option<(usize, &str)> {
    let mut index = 1;
    while let Some(arg) = argv.get(index) {
        match arg.as_str() {
            "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace" => index += 2,
            _ if arg.starts_with("--git-dir=")
                || arg.starts_with("--work-tree=")
                || arg.starts_with("--namespace=") =>
            {
                index += 1;
            }
            _ if arg.starts_with('-') => index += 1,
            _ => return Some((index, arg.as_str())),
        }
    }

    None
}

fn git_diff_uses_summary_output(args: &[String]) -> bool {
    args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--stat"
                | "--shortstat"
                | "--numstat"
                | "--name-only"
                | "--name-status"
                | "--summary"
                | "--raw"
                | "--check"
                | "--quiet"
        ) || arg.starts_with("--stat=")
            || arg.starts_with("--stat-width")
            || arg.starts_with("--stat-name-width")
            || arg.starts_with("--stat-count")
    })
}

fn is_search_command(cmd: &str) -> bool {
    let argv = command_argv(cmd);
    let argv = strip_known_command_wrappers(&argv);
    if argv_has_shell_operator(argv) {
        return false;
    }

    let Some(program) = argv
        .first()
        .and_then(|program| Path::new(program).file_name())
        .and_then(|program| program.to_str())
    else {
        return false;
    };

    if matches!(
        program,
        "rg" | "rga" | "ripgrep-all" | "grep" | "egrep" | "fgrep" | "ag" | "ack" | "pt"
    ) {
        return true;
    }

    program == "git" && git_subcommand(argv).is_some_and(|(_, subcommand)| subcommand == "grep")
}

fn command_argv(cmd: &str) -> Vec<String> {
    shlex::split(cmd).unwrap_or_else(|| cmd.split_whitespace().map(ToOwned::to_owned).collect())
}

fn argv_has_shell_operator(argv: &[String]) -> bool {
    argv.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "|" | "&&"
                | "||"
                | ";"
                | "<"
                | "<<"
                | "<<<"
                | ">"
                | ">>"
                | ">&"
                | "&>"
                | "&>>"
                | "1>"
                | "1>>"
                | "2>"
                | "2>>"
        ) || arg.starts_with('>')
            || arg.starts_with('<')
            || arg.starts_with("&>")
            || arg.starts_with("1>")
            || arg.starts_with("2>")
    })
}

fn args_imply_file_mutation(program: &str, args: &[String]) -> bool {
    match program {
        "yq" => args.iter().any(|arg| {
            arg == "-i"
                || arg == "--in-place"
                || arg == "--inplace"
                || arg.starts_with("--in-place=")
                || arg.starts_with("--inplace=")
        }),
        _ => false,
    }
}
