//! The TypeScript/JavaScript lexer (split from ts.rs, 1.1.8): code and
//! shape lines, strings, templates and regex literals blanked.

/// A TS/JS source read as code (0.6.5): `code` has comments removed and
/// template-literal text blanked (an `import … from` inside a template is
/// not an import); `shape` also blanks every string, template and regex
/// literal, so brace counting never trips on `'{'` or `"api/*"`. Both keep
/// the source's line structure.
pub struct Lexed {
    pub code: Vec<String>,
    pub shape: Vec<String>,
}

pub fn lex(src: &str) -> Lexed {
    #[derive(Clone, Copy, PartialEq)]
    enum S {
        Code,
        Line,
        Block,
        Str(char),
        Tmpl,
        /// inside a template's `${…}`: its brace depth (blanked, 0.7.7)
        TmplExpr(u32),
        Regex(bool), // inside a character class?
    }
    // after these words a `/` starts a regex, not a division (0.7.7)
    const REGEX_AFTER: &[&str] = &[
        "return",
        "typeof",
        "case",
        "in",
        "of",
        "delete",
        "void",
        "throw",
        "new",
        "else",
        "yield",
        "await",
        "instanceof",
        "do",
    ];
    let mut word = String::new();
    let chars: Vec<char> = src.chars().collect();
    let mut code = String::with_capacity(src.len());
    let mut shape = String::with_capacity(src.len());
    let mut s = S::Code;
    // the last significant code char: a `/` after one of these opens a regex
    let mut prev = '\n';
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied().unwrap_or('\0');
        if c == '\n' {
            code.push('\n');
            shape.push('\n');
            if matches!(s, S::Line) {
                s = S::Code;
            }
            if matches!(s, S::Str(_) | S::Regex(_)) {
                s = S::Code; // an unterminated literal ends at the line
            }
            i += 1;
            continue;
        }
        match s {
            S::Code => {
                if c == '/' && next == '/' {
                    s = S::Line;
                    i += 2;
                    continue;
                }
                if c == '/' && next == '*' {
                    s = S::Block;
                    i += 2;
                    continue;
                }
                // JSX: `</p>` closes a tag and `/>` ends one (0.7.7)
                let jsx = c == '/' && (prev == '<' || next == '>');
                let after_word = prev.is_alphanumeric() || prev == '_' || prev == '$';
                if c == '/'
                    && !jsx
                    && ("(,=:[!&|?{};+-*%<>~^\n".contains(prev)
                        || (after_word && REGEX_AFTER.contains(&word.as_str())))
                {
                    s = S::Regex(false);
                    code.push(c);
                    shape.push(' ');
                    i += 1;
                    continue;
                }
                if c == '\'' || c == '"' {
                    s = S::Str(c);
                    code.push(c);
                    shape.push(c);
                    i += 1;
                    continue;
                }
                if c == '`' {
                    s = S::Tmpl;
                    code.push(c);
                    shape.push(c);
                    i += 1;
                    continue;
                }
                code.push(c);
                shape.push(c);
                if c.is_alphanumeric() || c == '_' || c == '$' {
                    if !(prev.is_alphanumeric() || prev == '_' || prev == '$') {
                        word.clear();
                    }
                    word.push(c);
                }
                if !c.is_whitespace() {
                    prev = c;
                }
            }
            S::Line => {}
            S::Block => {
                if c == '*' && next == '/' {
                    s = S::Code;
                    i += 2;
                    code.push(' ');
                    shape.push(' ');
                    continue;
                }
            }
            S::Str(q) => {
                if c == '\\' {
                    code.push(c);
                    if next != '\n' && next != '\0' {
                        code.push(next);
                        shape.push(' ');
                        i += 1;
                    }
                    shape.push(' ');
                    i += 1;
                    continue;
                }
                code.push(c);
                if c == q {
                    shape.push(c);
                    s = S::Code;
                    prev = 'a'; // a string is an operand
                } else {
                    shape.push(' ');
                }
            }
            S::Tmpl => {
                if c == '\\' {
                    code.push(' ');
                    shape.push(' ');
                    if next != '\n' && next != '\0' {
                        code.push(' ');
                        shape.push(' ');
                        i += 1;
                    }
                    i += 1;
                    continue;
                }
                if c == '$' && next == '{' {
                    code.push(' ');
                    shape.push(' ');
                    code.push(' ');
                    shape.push(' ');
                    s = S::TmplExpr(1);
                    i += 2;
                    continue;
                }
                if c == '`' {
                    code.push(c);
                    shape.push(c);
                    s = S::Code;
                    prev = 'a';
                } else {
                    code.push(' ');
                    shape.push(' ');
                }
            }
            S::TmplExpr(depth) => {
                code.push(' ');
                shape.push(' ');
                match c {
                    '{' => s = S::TmplExpr(depth + 1),
                    '}' if depth == 1 => s = S::Tmpl,
                    '}' => s = S::TmplExpr(depth - 1),
                    _ => {}
                }
            }
            S::Regex(in_class) => {
                code.push(c);
                shape.push(' ');
                if c == '\\' {
                    if next != '\n' && next != '\0' {
                        code.push(next);
                        shape.push(' ');
                        i += 1;
                    }
                } else if c == '[' {
                    s = S::Regex(true);
                } else if c == ']' && in_class {
                    s = S::Regex(false);
                } else if c == '/' && !in_class {
                    s = S::Code;
                    prev = 'a';
                }
            }
        }
        i += 1;
    }
    Lexed {
        code: code.split('\n').map(str::to_string).collect(),
        shape: shape.split('\n').map(str::to_string).collect(),
    }
}

/// Comment-stripped lines (string-aware since 0.6.5: `"api/*"` or an URL
/// in a string no longer opens a comment).
pub fn code_lines(src: &str) -> Vec<String> {
    lex(src).code
}
