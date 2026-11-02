use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileRef {
    pub path: String,
    pub line: Option<usize>,
    pub symbol: Option<String>,
    pub label: Option<String>,
}

impl FileRef {
    pub fn display(&self) -> String {
        if let Some(label) = &self.label {
            return label.clone();
        }
        if let Some(symbol) = &self.symbol {
            return symbol.clone();
        }
        let base = self
            .path
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or(&self.path);
        match self.line {
            Some(line) => format!("{base}:{line}"),
            None => base.to_string(),
        }
    }

    pub fn resolved_path(&self) -> PathBuf {
        match self.path.strip_prefix("~/") {
            Some(rest) => home().join(rest),
            None if self.path == "~" => home(),
            None => PathBuf::from(&self.path),
        }
    }

    pub fn target_line(&self) -> Option<usize> {
        if self.line.is_some() {
            return self.line;
        }
        let symbol = self.symbol.as_deref()?;
        let source = std::fs::read_to_string(self.resolved_path()).ok()?;
        definition_line(&source, symbol)
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

pub(super) fn mention(code: &str) -> Option<FileRef> {
    let start = if code.starts_with("~/") {
        2
    } else if code.starts_with('/') {
        1
    } else {
        return None;
    };
    let end = code
        .find(|character: char| character.is_whitespace() || character == '#' || character == ':')
        .unwrap_or(code.len());
    let path = &code[..end];
    let (stem, extension) = path.rsplit_once('.')?;
    if stem.len() < start + 1
        || extension.is_empty()
        || !extension.chars().all(|character| character.is_ascii_alphanumeric())
    {
        return None;
    }
    let rest = &code[end..];
    let (line, symbol) = if rest.is_empty() {
        (None, None)
    } else if let Some(number) = rest.strip_prefix(':') {
        if number.is_empty() || !number.chars().all(|character| character.is_ascii_digit()) {
            return None;
        }
        (number.parse().ok(), None)
    } else if let Some(name) = rest.strip_prefix('#') {
        if !is_symbol(name) {
            return None;
        }
        (None, Some(name.to_string()))
    } else {
        return None;
    };
    Some(FileRef {
        path: path.to_string(),
        line,
        symbol,
        label: None,
    })
}

pub(super) fn file_link(href: &str, label: &str) -> Option<FileRef> {
    let rest = href.strip_prefix("file://")?;
    let (path, anchor) = match rest.split_once('#') {
        Some((path, anchor)) => (path, Some(anchor)),
        None => (rest, None),
    };
    if path.is_empty() {
        return None;
    }
    let path = decode(path);
    let (line, symbol) = match anchor {
        None | Some("") => (None, None),
        Some(anchor) => {
            let digits = anchor.strip_prefix('L').unwrap_or(anchor);
            if !digits.is_empty() && digits.chars().all(|character| character.is_ascii_digit()) {
                (digits.parse().ok(), None)
            } else if is_symbol(anchor) {
                (None, Some(anchor.to_string()))
            } else {
                (None, None)
            }
        }
    };
    let label = label.trim();
    Some(FileRef {
        path,
        line,
        symbol,
        label: (!label.is_empty()).then(|| label.to_string()),
    })
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if let Some(byte) = text
                .get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn is_identifier(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_' || first == '$')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '$')
}

fn is_symbol(name: &str) -> bool {
    !name.is_empty() && name.split('.').all(is_identifier)
}

const KEYWORDS: &[&str] = &[
    "fn", "function", "def", "class", "struct", "enum", "trait", "interface", "type", "func", "module", "object",
    "const", "let", "var", "impl", "mod",
];

pub fn definition_line(source: &str, symbol: &str) -> Option<usize> {
    let lines: Vec<&str> = source.lines().collect();
    let mut from = 0;
    let mut found = None;
    for part in symbol.split('.') {
        let index = (from..lines.len()).find(|&index| defines(lines[index], part))?;
        found = Some(index);
        from = index + 1;
    }
    found.map(|index| index + 1)
}

fn defines(line: &str, name: &str) -> bool {
    let words = words(line);
    for (position, word) in words.iter().enumerate() {
        if !KEYWORDS.contains(&word.text) {
            continue;
        }
        let after = &line[word.end..];
        let after = if word.text == "func" { skip_receiver(after) } else { after };
        let after = if word.text == "impl" { skip_generics(after) } else { after };
        if leading_identifier(after) == Some(name) {
            return true;
        }
        if word.text == "impl" && words[position + 1..].iter().any(|later| later.text == "for") {
            if let Some(target) = line.split(" for ").nth(1) {
                if leading_identifier(target.trim_start()) == Some(name) {
                    return true;
                }
            }
        }
    }
    method(line, name)
}

struct Word<'a> {
    text: &'a str,
    end: usize,
}

fn words(line: &str) -> Vec<Word<'_>> {
    let mut found = Vec::new();
    let mut start = None;
    for (index, character) in line.char_indices() {
        let part = character.is_ascii_alphanumeric() || character == '_' || character == '$';
        match (part, start) {
            (true, None) => start = Some(index),
            (false, Some(begin)) => {
                found.push(Word {
                    text: &line[begin..index],
                    end: index,
                });
                start = None;
            }
            _ => {}
        }
    }
    if let Some(begin) = start {
        found.push(Word {
            text: &line[begin..],
            end: line.len(),
        });
    }
    found
}

fn leading_identifier(text: &str) -> Option<&str> {
    let text = text.trim_start();
    let end = text
        .find(|character: char| !(character.is_ascii_alphanumeric() || character == '_' || character == '$'))
        .unwrap_or(text.len());
    let name = &text[..end];
    is_identifier(name).then_some(name)
}

fn skip_receiver(text: &str) -> &str {
    let trimmed = text.trim_start();
    if !trimmed.starts_with('(') {
        return text;
    }
    match trimmed.find(')') {
        Some(close) => &trimmed[close + 1..],
        None => text,
    }
}

fn skip_generics(text: &str) -> &str {
    let trimmed = text.trim_start();
    if !trimmed.starts_with('<') {
        return text;
    }
    let mut depth = 0;
    for (index, character) in trimmed.char_indices() {
        match character {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return &trimmed[index + 1..];
                }
            }
            _ => {}
        }
    }
    text
}

const MODIFIERS: &[&str] = &["public", "private", "protected", "static", "async", "override", "export", "pub"];
const NOT_METHODS: &[&str] = &["if", "for", "while", "switch", "catch", "return", "match", "with"];

fn method(line: &str, name: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.ends_with('{') || NOT_METHODS.contains(&name) {
        return false;
    }
    let mut rest = trimmed;
    loop {
        let Some(word) = leading_identifier(rest) else {
            return false;
        };
        if MODIFIERS.contains(&word) {
            rest = rest.trim_start()[word.len()..].trim_start();
            continue;
        }
        return word == name && rest.trim_start()[word.len()..].trim_start().starts_with('(');
    }
}

#[cfg(test)]
mod tests {
    use super::{definition_line, file_link, mention, FileRef};

    fn reference(path: &str, line: Option<usize>, symbol: Option<&str>) -> FileRef {
        FileRef {
            path: path.to_string(),
            line,
            symbol: symbol.map(str::to_string),
            label: None,
        }
    }

    #[test]
    fn mentions_take_full_paths_with_an_extension() {
        assert_eq!(mention("/src/app.rs"), Some(reference("/src/app.rs", None, None)));
        assert_eq!(mention("~/src/app.rs:42"), Some(reference("~/src/app.rs", Some(42), None)));
        assert_eq!(
            mention("~/src/create.ts#Store.add"),
            Some(reference("~/src/create.ts", None, Some("Store.add")))
        );
        assert_eq!(mention("src/app.rs"), None);
        assert_eq!(mention("/usr/bin"), None);
        assert_eq!(mention("~/src/app.rs:x"), None);
        assert_eq!(mention("/a b.rs"), None);
        assert_eq!(mention("cargo test"), None);
    }

    #[test]
    fn file_links_read_lines_and_symbols_from_the_anchor() {
        let found = file_link("file:///Users/r/src/a%20b.rs#L12", "the handler").unwrap();
        assert_eq!(found.path, "/Users/r/src/a b.rs");
        assert_eq!(found.line, Some(12));
        assert_eq!(found.label.as_deref(), Some("the handler"));
        assert_eq!(file_link("file:///x/y.ts#createOrder", "").unwrap().symbol.as_deref(), Some("createOrder"));
        assert_eq!(file_link("https://example.com", "x"), None);
    }

    #[test]
    fn display_prefers_the_label_then_the_symbol_then_the_file_name() {
        assert_eq!(reference("/a/b.rs", Some(3), None).display(), "b.rs:3");
        assert_eq!(reference("/a/b.rs", None, Some("run")).display(), "run");
    }

    #[test]
    fn definitions_are_found_by_keyword_method_and_nesting() {
        let source = "use x;\n\npub struct Store {\n}\n\nimpl Store {\n    pub fn add(&self) {\n    }\n}\n\nexport async function createOrder(input) {\n}\nclass Cart {\n  total(items) {\n  }\n}\nfunc (s *Server) Serve() {\n}\n";
        assert_eq!(definition_line(source, "Store"), Some(3));
        assert_eq!(definition_line(source, "Store.add"), Some(7));
        assert_eq!(definition_line(source, "createOrder"), Some(11));
        assert_eq!(definition_line(source, "Cart.total"), Some(14));
        assert_eq!(definition_line(source, "Serve"), Some(17));
        assert_eq!(definition_line(source, "missing"), None);
    }
}
