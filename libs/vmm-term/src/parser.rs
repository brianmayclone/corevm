//! Line parser — splits input into command name + arguments.
//! Supports quoted strings: vm-start "My VM Name"

/// Parse a command line into (command_name, arguments).
/// Returns None for empty/whitespace-only input.
pub fn parse_line(input: &str) -> Option<(String, Vec<String>)> {
    let trimmed = input.trim();
    if trimmed.is_empty() { return None; }

    let tokens = tokenize(trimmed);
    if tokens.is_empty() { return None; }

    let name = tokens[0].to_lowercase();
    let args: Vec<String> = tokens[1..].to_vec();
    Some((name, args))
}

/// Tokenize input respecting quoted strings.
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut quote_char = '"';

    for ch in input.chars() {
        if in_quote {
            if ch == quote_char {
                in_quote = false;
            } else {
                current.push(ch);
            }
        } else {
            match ch {
                '"' | '\'' => {
                    in_quote = true;
                    quote_char = ch;
                }
                ' ' | '\t' => {
                    if !current.is_empty() {
                        tokens.push(std::mem::take(&mut current));
                    }
                }
                _ => current.push(ch),
            }
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple() {
        let (name, args) = parse_line("vm-list").unwrap();
        assert_eq!(name, "vm-list");
        assert!(args.is_empty());
    }

    #[test]
    fn test_args() {
        let (name, args) = parse_line("vm-start abc123").unwrap();
        assert_eq!(name, "vm-start");
        assert_eq!(args, vec!["abc123"]);
    }

    #[test]
    fn test_quoted() {
        let (name, args) = parse_line(r#"vm-create "My VM" 4 8192"#).unwrap();
        assert_eq!(name, "vm-create");
        assert_eq!(args, vec!["My VM", "4", "8192"]);
    }

    #[test]
    fn test_empty() {
        assert!(parse_line("").is_none());
        assert!(parse_line("   ").is_none());
    }
}
