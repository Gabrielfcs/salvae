//! Minimal Valve KeyValues (VDF) parser.
//!
//! Handles the subset used by Steam's `.acf` and `libraryfolders.vdf`: quoted
//! string keys/values, `{ }` nested objects, `//` line comments, and the
//! escapes `\\`, `\"`, `\n`, `\t`.

use std::collections::BTreeMap;

use crate::DetectError;

/// A parsed VDF value: either a string or a nested object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Vdf {
    Str(String),
    Obj(BTreeMap<String, Vdf>),
}

impl Vdf {
    /// Borrow as an object, if it is one.
    pub fn as_obj(&self) -> Option<&BTreeMap<String, Vdf>> {
        match self {
            Vdf::Obj(m) => Some(m),
            Vdf::Str(_) => None,
        }
    }

    /// Borrow as a string, if it is one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Vdf::Str(s) => Some(s),
            Vdf::Obj(_) => None,
        }
    }
}

/// Parse a VDF document into a top-level object.
pub fn parse(input: &str) -> Result<Vdf, DetectError> {
    let tokens = tokenize(input)?;
    let mut pos = 0;
    let obj = parse_obj_body(&tokens, &mut pos, false)?;
    if pos != tokens.len() {
        return Err(DetectError::Parse("trailing tokens after document".into()));
    }
    Ok(Vdf::Obj(obj))
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Str(String),
    Open,
    Close,
}

fn tokenize(input: &str) -> Result<Vec<Token>, DetectError> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            c if c.is_whitespace() => {
                chars.next();
            }
            '{' => {
                chars.next();
                tokens.push(Token::Open);
            }
            '}' => {
                chars.next();
                tokens.push(Token::Close);
            }
            '/' => {
                chars.next();
                if chars.peek() == Some(&'/') {
                    // line comment: skip to end of line
                    for n in chars.by_ref() {
                        if n == '\n' {
                            break;
                        }
                    }
                } else {
                    return Err(DetectError::Parse("unexpected '/'".into()));
                }
            }
            '"' => {
                chars.next(); // opening quote
                let mut s = String::new();
                loop {
                    match chars.next() {
                        None => return Err(DetectError::Parse("unterminated string".into())),
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some('\\') => s.push('\\'),
                            Some('"') => s.push('"'),
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(other) => s.push(other),
                            None => return Err(DetectError::Parse("dangling escape".into())),
                        },
                        Some(other) => s.push(other),
                    }
                }
                tokens.push(Token::Str(s));
            }
            other => {
                return Err(DetectError::Parse(format!(
                    "unexpected character {other:?}"
                )));
            }
        }
    }
    Ok(tokens)
}

/// Parse the body of an object (a sequence of key/value pairs). If `expect_close`
/// is true, consume a matching `}` at the end.
fn parse_obj_body(
    tokens: &[Token],
    pos: &mut usize,
    expect_close: bool,
) -> Result<BTreeMap<String, Vdf>, DetectError> {
    let mut map = BTreeMap::new();
    loop {
        match tokens.get(*pos) {
            None => {
                if expect_close {
                    return Err(DetectError::Parse("unbalanced braces".into()));
                }
                break;
            }
            Some(Token::Close) => {
                if expect_close {
                    *pos += 1; // consume '}'
                    break;
                }
                return Err(DetectError::Parse("unexpected '}'".into()));
            }
            Some(Token::Open) => return Err(DetectError::Parse("expected key, found '{'".into())),
            Some(Token::Str(key)) => {
                let key = key.clone();
                *pos += 1;
                match tokens.get(*pos) {
                    Some(Token::Str(value)) => {
                        map.insert(key, Vdf::Str(value.clone()));
                        *pos += 1;
                    }
                    Some(Token::Open) => {
                        *pos += 1; // consume '{'
                        let child = parse_obj_body(tokens, pos, true)?;
                        map.insert(key, Vdf::Obj(child));
                    }
                    _ => return Err(DetectError::Parse(format!("key {key:?} missing value"))),
                }
            }
        }
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flat_keys_and_values() {
        let input = r#"
            "AppState"
            {
                "appid"  "892970"
                "name"   "Valheim"
            }
        "#;
        let v = parse(input).unwrap();
        let app = v
            .as_obj()
            .unwrap()
            .get("AppState")
            .unwrap()
            .as_obj()
            .unwrap();
        assert_eq!(app.get("appid").unwrap().as_str(), Some("892970"));
        assert_eq!(app.get("name").unwrap().as_str(), Some("Valheim"));
    }

    #[test]
    fn parses_nested_objects_and_windows_paths() {
        let input = r#"
            "libraryfolders"
            {
                "0" { "path" "C:\\Program Files\\Steam" }
                "1" { "path" "D:\\Games\\SteamLibrary" }
            }
        "#;
        let v = parse(input).unwrap();
        let lf = v
            .as_obj()
            .unwrap()
            .get("libraryfolders")
            .unwrap()
            .as_obj()
            .unwrap();
        assert_eq!(
            lf.get("1")
                .unwrap()
                .as_obj()
                .unwrap()
                .get("path")
                .unwrap()
                .as_str(),
            Some("D:\\Games\\SteamLibrary")
        );
    }

    #[test]
    fn skips_line_comments() {
        let input = "\"root\" {\n  // a comment\n  \"k\" \"v\"\n}\n";
        let v = parse(input).unwrap();
        let root = v.as_obj().unwrap().get("root").unwrap().as_obj().unwrap();
        assert_eq!(root.get("k").unwrap().as_str(), Some("v"));
    }

    #[test]
    fn unbalanced_braces_error() {
        assert!(matches!(parse("\"a\" {"), Err(DetectError::Parse(_))));
    }
}
