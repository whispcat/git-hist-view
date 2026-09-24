use rustc_hash::FxHashMap;

#[derive(Debug, Clone)]
pub struct Author {
    pub name: String,
    pub email: String,
    pub commits: u32,
    pub bot: bool,
}

const BOTS: &[&str] = &[
    "dependabot",
    "renovate",
    "renovate-bot",
    "greenkeeper",
    "snyk-bot",
    "github-actions",
    "semantic-release-bot",
    "pre-commit-ci",
    "allcontributors",
];

/// GitHub apps commit as `name[bot]`; a few long-lived bot accounts predate that convention.
pub fn is_bot(name: &str, email: &str) -> bool {
    let (name, email) = (name.to_lowercase(), email.to_lowercase());
    let local = email.split('@').next().unwrap_or_default();
    let local = local.rsplit('+').next().unwrap_or(local);
    name.contains("[bot]") || email.contains("[bot]") || BOTS.contains(&name.as_str()) || BOTS.contains(&local)
}

type MailmapKey = (String, Option<String>);

/// Git `.mailmap`: maps a commit identity (email, optionally name) to a canonical name and/or email.
#[derive(Default)]
pub struct Mailmap(FxHashMap<MailmapKey, (Option<String>, Option<String>)>);

impl Mailmap {
    pub fn parse(data: &[u8]) -> Self {
        let mut map = FxHashMap::default();
        for line in String::from_utf8_lossy(data).lines() {
            let line = line.split('#').next().unwrap_or_default();
            let mut parts = Vec::new();
            let mut rest = line;
            while let (Some(lt), Some(gt)) = (rest.find('<'), rest.find('>')) {
                if gt < lt {
                    break;
                }
                parts.push((rest[..lt].trim(), rest[lt + 1..gt].trim()));
                rest = &rest[gt + 1..];
            }
            let non_empty = |s: &str| (!s.is_empty()).then(|| s.to_string());
            match parts.as_slice() {
                [(name, email)] => {
                    map.insert((email.to_lowercase(), None), (non_empty(name), None));
                }
                [(name, email), (commit_name, commit_email)] => {
                    let key = (commit_email.to_lowercase(), non_empty(commit_name).map(|n| n.to_lowercase()));
                    map.insert(key, (non_empty(name), non_empty(email)));
                }
                _ => {}
            }
        }
        Mailmap(map)
    }

    fn resolve(&self, name: String, email: String) -> (String, String) {
        let email_key = email.to_lowercase();
        let hit = self.0.get(&(email_key.clone(), Some(name.to_lowercase()))).or_else(|| self.0.get(&(email_key, None)));
        match hit {
            Some((n, e)) => (n.clone().unwrap_or(name), e.clone().unwrap_or(email)),
            None => (name, email),
        }
    }
}

/// Folds case and GitHub noreply aliases (`123+user@users.noreply.github.com`) into one identity.
pub fn identity_key(name: &str, email: &str) -> String {
    let email = email.to_lowercase();
    match email.strip_suffix("@users.noreply.github.com") {
        Some(user) => format!("gh:{}", user.rsplit('+').next().unwrap_or(user)),
        None if email.is_empty() => format!("name:{}", name.to_lowercase()),
        None => email,
    }
}

#[derive(Default)]
pub struct Authors {
    pub list: Vec<Author>,
    by_key: FxHashMap<String, u32>,
    mailmap: Mailmap,
}

impl Authors {
    pub fn with_mailmap(mailmap: Mailmap) -> Self {
        Authors { mailmap, ..Default::default() }
    }

    /// Returns the author id without counting a commit (used for side-branch attribution).
    pub fn lookup(&mut self, name: &[u8], email: &[u8]) -> u32 {
        let (name, email) = self.mailmap.resolve(String::from_utf8_lossy(name).into(), String::from_utf8_lossy(email).into());
        let key = identity_key(&name, &email);
        *self.by_key.entry(key).or_insert_with(|| {
            let bot = is_bot(&name, &email);
            self.list.push(Author { name, email, commits: 0, bot });
            self.list.len() as u32 - 1
        })
    }

    pub fn intern(&mut self, name: &[u8], email: &[u8]) -> u32 {
        let id = self.lookup(name, email);
        self.list[id as usize].commits += 1;
        id
    }

    pub fn find(&self, name: &str, email: &str) -> Option<u32> {
        let (name, email) = self.mailmap.resolve(name.into(), email.into());
        self.by_key.get(&identity_key(&name, &email)).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailmap_and_noreply_fold_identities() {
        let mm = Mailmap::parse(b"Ada Lovelace <ada@example.com> <ada@old.org>\nBob <bob@example.com> # comment\n");
        let mut a = Authors::with_mailmap(mm);
        let x = a.intern(b"ada", b"ADA@old.org");
        let y = a.intern(b"Ada L", b"ada@example.com");
        assert_eq!(x, y);
        assert_eq!(a.list[x as usize].name, "Ada Lovelace");
        let g1 = a.intern(b"cy", b"1+cy@users.noreply.github.com");
        let g2 = a.intern(b"Cy", b"cy@users.noreply.github.com");
        assert_eq!(g1, g2);
    }

    #[test]
    fn recognizes_bots() {
        assert!(is_bot("renovate[bot]", "29139614+renovate[bot]@users.noreply.github.com"));
        assert!(is_bot("dependabot-preview", "27856297+dependabot-preview[bot]@users.noreply.github.com"));
        assert!(is_bot("Renovate Bot", "renovate@whitesourcesoftware.com"));
        assert!(!is_bot("Ada Lovelace", "ada@example.com"));
        assert!(!is_bot("Robert", "bob@example.com"));
    }
}
