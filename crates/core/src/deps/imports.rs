/// One import as extracted by `ghv-parse` (see its record format).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Import {
    Js(String),
    Mod { name: String, path: Option<String> },
    Use(Vec<String>),
    Py { level: usize, module: Vec<String>, names: Vec<String> },
}

pub fn parse_records(text: &str) -> Vec<Import> {
    let split = |s: &str, sep| s.split(sep).filter(|p: &&str| !p.is_empty()).map(String::from).collect();
    text.lines()
        .filter_map(|line| {
            let (tag, rest) = line.split_once(' ')?;
            Some(match tag {
                "j" => Import::Js(rest.into()),
                "m" => Import::Mod { name: rest.into(), path: None },
                "M" => {
                    let (name, path) = rest.split_once(' ')?;
                    Import::Mod { name: name.into(), path: Some(path.into()) }
                }
                "u" => Import::Use(split(rest, "::")),
                "i" => Import::Py { level: 0, module: split(rest, "."), names: Vec::new() },
                "f" => {
                    let mut parts = rest.splitn(3, ' ');
                    let level = parts.next()?.parse().ok()?;
                    let module = parts.next()?;
                    let names = parts.next().map_or_else(Vec::new, |n| split(n, ","));
                    Import::Py { level, module: if module == "-" { Vec::new() } else { split(module, ".") }, names }
                }
                _ => return None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_extractor_records() {
        let imports = parse_records("j ./x\nM a other/p.rs\nm b\nu crate::x::y\ni a.b\nf 2 f.g h,i\nf 1 - e");
        assert_eq!(imports[1], Import::Mod { name: "a".into(), path: Some("other/p.rs".into()) });
        assert_eq!(imports[3], Import::Use(vec!["crate".into(), "x".into(), "y".into()]));
        assert_eq!(imports[5], Import::Py { level: 2, module: vec!["f".into(), "g".into()], names: vec!["h".into(), "i".into()] });
        assert_eq!(imports[6], Import::Py { level: 1, module: vec![], names: vec!["e".into()] });
    }
}
