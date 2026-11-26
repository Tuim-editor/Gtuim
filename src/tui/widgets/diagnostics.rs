//! Location extraction from common compiler output. Paths are never shell-evaluated.
use super::compile::CompileLine;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

pub fn parse_location(text: &str) -> Option<Diagnostic> {
    let text = text
        .trim()
        .trim_start_matches("--> ")
        .trim_start_matches("::: ");
    // Python traceback: File "path", line N
    if let Some(rest) = text.strip_prefix("File \"") {
        let (path, rest) = rest.split_once("\", line ")?;
        let line = rest
            .split(|c: char| !c.is_ascii_digit())
            .next()?
            .parse::<usize>()
            .ok()?;
        return (line > 0).then(|| Diagnostic {
            path: path.into(),
            line,
            column: 1,
            message: text.into(),
        });
    }
    // GCC, Clang, rustc, Go, TypeScript: path:line[:column]: message
    for (index, _) in text.match_indices(':') {
        let path = text[..index].trim();
        if path.is_empty()
            || path.contains("://")
            || path.chars().all(|c| c.is_ascii_digit() || c == ':')
        {
            continue;
        }
        let rest = &text[index + 1..];
        let (line_text, tail) = rest.split_once(':').unwrap_or((rest, ""));
        let Ok(line) = line_text.parse::<usize>() else {
            continue;
        };
        if line == 0 {
            continue;
        }
        let (column, message) = match tail.split_once(':') {
            Some((col, message)) if col.parse::<usize>().is_ok() => {
                (col.parse::<usize>().unwrap().max(1), message.trim())
            }
            _ if tail
                .split_once(" - ")
                .is_some_and(|(col, _)| col.parse::<usize>().is_ok()) =>
            {
                let (col, message) = tail.split_once(" - ").unwrap();
                (col.parse::<usize>().unwrap().max(1), message.trim())
            }
            _ if tail.parse::<usize>().is_ok() => (tail.parse::<usize>().unwrap().max(1), ""),
            _ => (1, tail.trim()),
        };
        return Some(Diagnostic {
            path: path.into(),
            line,
            column,
            message: message.into(),
        });
    }
    // MSVC / dotnet: path(line,column): message
    for (end, _) in text.match_indices("):") {
        let Some((path, numbers)) = text[..end].rsplit_once('(') else {
            continue;
        };
        let (line, col) = numbers.split_once(',').unwrap_or((numbers, "1"));
        let (Ok(line), Ok(column)) = (line.parse::<usize>(), col.parse::<usize>()) else {
            continue;
        };
        if line > 0 && !path.trim().is_empty() {
            return Some(Diagnostic {
                path: path.trim().into(),
                line,
                column: column.max(1),
                message: text[end + 2..].trim().into(),
            });
        }
    }
    None
}

pub fn collect(lines: &[CompileLine]) -> Vec<Diagnostic> {
    let mut result = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if let Some(mut diagnostic) = parse_location(&line.plain) {
            if diagnostic.message.is_empty() {
                diagnostic.message = lines[..index]
                    .iter()
                    .rev()
                    .take(4)
                    .find(|line| {
                        line.plain.trim_start().starts_with("error")
                            || line.plain.trim_start().starts_with("warning")
                    })
                    .map(|line| line.plain.trim().to_owned())
                    .unwrap_or_else(|| "Compiler location".into());
            }
            result.push(diagnostic);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{collect, parse_location};
    use crate::tui::widgets::compile::parse_ansi_line;
    #[test]
    fn common_compiler_locations_and_paths_with_spaces() {
        for (text, path, line, col) in [
            ("src/a b.c:12:4: error: bad type", "src/a b.c", 12, 4),
            (" --> src/main.rs:8:2", "src/main.rs", 8, 2),
            ("main.go:19: undefined name", "main.go", 19, 1),
            (
                "  File \"app test.py\", line 7, in run",
                "app test.py",
                7,
                1,
            ),
            ("src/Test.cs(10,5): error CS1002", "src/Test.cs", 10, 5),
        ] {
            let d = parse_location(text).expect(text);
            assert_eq!((d.path.as_str(), d.line, d.column), (path, line, col));
        }
        assert!(parse_location("Finished in 2 seconds").is_none());
        assert!(parse_location("file.c:0: bad").is_none());
    }
    #[test]
    fn rust_multiline_keeps_error_message_and_strips_ansi() {
        let result = collect(&[
            parse_ansi_line("\x1b[31merror[E0308]: mismatched types\x1b[0m"),
            parse_ansi_line(" --> src/main.rs:4:8"),
        ]);
        assert_eq!(result[0].message, "error[E0308]: mismatched types");
    }
}
