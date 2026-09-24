#!/usr/bin/env bash
# Builds deterministic synthetic repos under fixtures/synthetic for golden tests.
set -euo pipefail
root="$(cd "$(dirname "$0")" && pwd)/synthetic"
rm -rf "$root" && mkdir -p "$root"
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1

tick=1700000000
commit() { # author, message
  tick=$((tick + 3600))
  local name="${1%%:*}" email="${1##*:}"
  GIT_AUTHOR_NAME="$name" GIT_AUTHOR_EMAIL="$email" GIT_AUTHOR_DATE="@$tick +0000" \
  GIT_COMMITTER_NAME="$name" GIT_COMMITTER_EMAIL="$email" GIT_COMMITTER_DATE="@$tick +0000" \
    git commit -q --allow-empty -m "$2"
}
ada="Ada Lovelace:ada@example.com"
bob="Bob Builder:bob@example.com"
cy="Cy Twombly:12345+cy@users.noreply.github.com"

git init -q -b main "$root/basic" && cd "$root/basic"
mkdir -p src docs
printf 'fn main() {\n    println!("hi");\n}\n' > src/main.rs
printf '# Title\n\nIntro\n' > docs/README.md
printf 'no trailing newline' > notes.txt
: > empty.txt
git add -A && commit "$ada" "initial"

printf 'fn main() {\n    greet();\n}\n\nfn greet() {\n    println!("hello");\n}\n' > src/main.rs
printf 'line one\r\nline two\r\n' > crlf.txt
head -c 2048 /dev/zero > blob.bin
git add -A && commit "$bob" "add greet, crlf, binary"

printf 'no trailing newline\n' > notes.txt
ln -s src/main.rs link.rs
git add -A && commit "$cy" "fix newline, add symlink"

git switch -q -c feature
printf 'pub fn util() -> u32 {\n    42\n}\n' > src/util.rs
git add -A && commit "$bob" "feature: util"
printf 'pub fn util() -> u32 {\n    43\n}\n\npub fn more() {}\n' > src/util.rs
git add -A && commit "$cy" "feature: more"
git switch -q main
printf '# Title\n\nIntro\n\n## Usage\n\nRun it.\n' > docs/README.md
git add -A && commit "$ada" "docs usage"
GIT_AUTHOR_DATE="@$((tick + 1800)) +0000" GIT_COMMITTER_DATE="@$((tick + 1800)) +0000" \
  GIT_AUTHOR_NAME=Ada GIT_AUTHOR_EMAIL=ada@example.com GIT_COMMITTER_NAME=Ada GIT_COMMITTER_EMAIL=ada@example.com \
  git merge -q --no-ff feature -m "merge feature"
tick=$((tick + 3600))

git mv src/util.rs src/helpers.rs
git add -A && commit "$ada" "rename util"

git rm -q empty.txt && mkdir notes && mv notes.txt notes/today.txt && rm -f notes.txt
git add -A && commit "$bob" "file to dir"

git update-index --add --cacheinfo 160000,1111111111111111111111111111111111111111,vendor/sub
commit "$cy" "add submodule"

for i in $(seq 1 30); do
  seq 1 $((i * 3)) > "src/gen.txt"
  printf 'fn main() {\n    greet();\n    step(%d);\n}\n\nfn greet() {\n    println!("hello");\n}\n' "$i" > src/main.rs
  git add -A && commit "$([ $((i % 3)) = 0 ] && echo "$ada" || echo "$bob")" "step $i"
done
bot="renovate[bot]:29139614+renovate[bot]@users.noreply.github.com"
printf '1\n2\n3\n' > src/gen.txt && printf 'fn main() {}\n' > src/main.rs
git add -A && commit "$bot" "chore(deps): bot touches the coupled pair"
mkdir -p web
for v in 1 2 3; do
  printf '{ "version": "0.%d.0" }\n' "$v" > package.json && cp package.json web/package.json
  git add -A && commit "$bob" "release 0.$v.0"
done
mkdir -p web/lib py/pkg
printf "import { util } from './lib/util';\nexport const app = util;\n" > web/app.ts
printf 'export const util = 1;\n' > web/lib/util.ts
printf 'from .core import x\n' > py/pkg/__init__.py
printf 'x = 1\n' > py/pkg/core.py
git add -A && commit "$ada" "add imports"
git gc -q --prune=now
printf 'loose\n' > loose.txt && git add loose.txt && commit "$ada" "loose object"
echo "built $(git rev-list --count HEAD) commits in $root/basic"
