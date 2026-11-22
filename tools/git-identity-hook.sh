#!/bin/bash
# Identity gate: commits must use the identity configured in the repo's git
# config FILES (repo-local `.git/config`, falling back to `--global`), so
# `-c`/env overrides cannot redefine the pin. `git var` resolves identity
# exactly as the commit would (config/env/-c), so ad-hoc identities are
# caught regardless of source.
set -euo pipefail
gitdir=$(git rev-parse --git-dir)
name=$(git config --file "$gitdir/config" user.name 2>/dev/null || git config --global user.name 2>/dev/null || true)
email=$(git config --file "$gitdir/config" user.email 2>/dev/null || git config --global user.email 2>/dev/null || true)
if [ -z "$name" ] || [ -z "$email" ]; then
  echo "pre-commit: no identity found in git config files (repo-local or global)" >&2
  exit 1
fi
want="$name <$email>"
bad=0
for which in AUTHOR COMMITTER; do
  ident=$(git var "GIT_${which}_IDENT")
  # strip trailing timestamp + tz: "Name <email> 1713... +0800"
  resolved=$(sed -E 's/ [0-9]+ [+-][0-9]{4}$//' <<<"$ident")
  if [ "$resolved" != "$want" ]; then
    echo "pre-commit: $which identity '$resolved' != configured '$want'" >&2
    bad=1
  fi
done
if [ "$bad" = 1 ]; then
  echo "fix: commit with the identity configured in git config (user.name/user.email)" >&2
  exit 1
fi
exit 0
