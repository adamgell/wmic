//! Windows-style command-line text: argument quoting and splitting.

/// Quotes an argument when it is empty or holds white space or a double quote.
pub fn quote(argument: &str) -> String {
    if !argument.is_empty() && !argument.contains([' ', '\t', '"']) {
        return argument.to_owned();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0usize;
    for c in argument.chars() {
        match c {
            '\\' => {
                backslashes += 1;
                out.push('\\');
            }
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                backslashes = 0;
                out.push(c);
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes));
    out.push('"');
    out
}

/// Joins arguments with single spaces, quoting where needed.
pub fn join<S: AsRef<str>>(arguments: &[S]) -> String {
    arguments
        .iter()
        .map(|a| quote(a.as_ref()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Splits a command line without a shell: double quotes group, and backslashes escape a quote only
/// when they directly precede it (2n backslashes yield n and toggle quoting, 2n+1 yield n and a
/// literal quote).
pub fn split(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut arguments = vec![];
    let mut at = 0;
    loop {
        while at < chars.len() && matches!(chars[at], ' ' | '\t') {
            at += 1;
        }
        if at >= chars.len() {
            return arguments;
        }
        let mut argument = String::new();
        let mut quoted = false;
        while at < chars.len() {
            match chars[at] {
                '\\' => {
                    let run = chars[at..].iter().take_while(|c| **c == '\\').count();
                    at += run;
                    if chars.get(at) == Some(&'"') {
                        argument.extend(std::iter::repeat_n('\\', run / 2));
                        if run % 2 == 1 {
                            argument.push('"');
                        } else {
                            quoted = !quoted;
                        }
                        at += 1;
                    } else {
                        argument.extend(std::iter::repeat_n('\\', run));
                    }
                }
                '"' => {
                    quoted = !quoted;
                    at += 1;
                }
                ' ' | '\t' if !quoted => break,
                c => {
                    argument.push(c);
                    at += 1;
                }
            }
        }
        arguments.push(argument);
    }
}
