//! A small lenient JSON reader for config files: accepts comments and trailing commas (JSONC, as in tsconfig).

#[derive(Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn entries(&self) -> &[(String, Json)] {
        match self {
            Json::Obj(fields) => fields,
            _ => &[],
        }
    }

    pub fn items(&self) -> &[Json] {
        match self {
            Json::Arr(items) => items,
            _ => &[],
        }
    }
}

struct Reader<'a> {
    src: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn skip(&mut self) {
        loop {
            while self.src.get(self.pos).is_some_and(u8::is_ascii_whitespace) {
                self.pos += 1;
            }
            match (self.src.get(self.pos), self.src.get(self.pos + 1)) {
                (Some(b'/'), Some(b'/')) => {
                    while self.src.get(self.pos).is_some_and(|&c| c != b'\n') {
                        self.pos += 1;
                    }
                }
                (Some(b'/'), Some(b'*')) => {
                    self.pos += 2;
                    while self.pos < self.src.len() && !self.src[self.pos..].starts_with(b"*/") {
                        self.pos += 1;
                    }
                    self.pos += 2;
                }
                _ => return,
            }
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.src.get(self.pos).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        let hit = self.peek() == Some(c);
        self.pos += usize::from(hit);
        hit
    }

    fn string(&mut self) -> Option<String> {
        self.pos += 1;
        let mut out = Vec::new();
        loop {
            let c = *self.src.get(self.pos)?;
            self.pos += 1;
            match c {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let e = *self.src.get(self.pos)?;
                    self.pos += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let hex = std::str::from_utf8(self.src.get(self.pos..self.pos + 4)?).ok()?;
                            let ch = char::from_u32(u32::from_str_radix(hex, 16).ok()?).unwrap_or('\u{fffd}');
                            out.extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes());
                            self.pos += 4;
                        }
                        other => out.push(other),
                    }
                }
                c => out.push(c),
            }
        }
    }

    fn value(&mut self) -> Option<Json> {
        Some(match self.peek()? {
            b'{' => {
                self.pos += 1;
                let mut fields = Vec::new();
                while !self.eat(b'}') {
                    if self.peek()? != b'"' {
                        return None;
                    }
                    let key = self.string()?;
                    if !self.eat(b':') {
                        return None;
                    }
                    fields.push((key, self.value()?));
                    self.eat(b',');
                }
                Json::Obj(fields)
            }
            b'[' => {
                self.pos += 1;
                let mut items = Vec::new();
                while !self.eat(b']') {
                    items.push(self.value()?);
                    self.eat(b',');
                }
                Json::Arr(items)
            }
            b'"' => Json::Str(self.string()?),
            _ => {
                let start = self.pos;
                while self.src.get(self.pos).is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'+' | b'.')) {
                    self.pos += 1;
                }
                match &self.src[start..self.pos] {
                    b"true" => Json::Bool(true),
                    b"false" => Json::Bool(false),
                    b"null" => Json::Null,
                    n => Json::Num(std::str::from_utf8(n).ok()?.parse().ok()?),
                }
            }
        })
    }
}

pub fn parse(src: &str) -> Option<Json> {
    Reader { src: src.as_bytes(), pos: 0 }.value()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_tsconfig_style_jsonc() {
        let src = r#"{
            // comment
            "extends": "./base.json", /* block */
            "compilerOptions": { "baseUrl": ".", "paths": { "@/*": ["src/*",], }, "strict": true, "n": -1.5e3 },
        }"#;
        let json = parse(src).unwrap();
        assert_eq!(json.get("extends").and_then(Json::as_str), Some("./base.json"));
        let paths = json.get("compilerOptions").and_then(|c| c.get("paths")).unwrap();
        assert_eq!(paths.entries()[0].1.items()[0].as_str(), Some("src/*"));
    }
}
