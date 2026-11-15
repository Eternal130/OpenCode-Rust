#!/bin/bash
# Identity gate: repo discipline pins Eternal130 <Eternal130@outlook.com>.
# `git var` resolves identity exactly as the commit would (config/env/-c),
# so ad-hoc subagent identities are caught regardless of source.
set -euo pipefail
want="Eternal130 <Eternal130@outlook.com>"
bad=0
for which in AUTHOR COMMITTER; do
  ident=$(git var "GIT_${which}_IDENT")
  # strip trailing timestamp + tz: "Name <email> 1713... +0800"
  resolved=$(sed -E 's/ [0-9]+ [+-][0-9]{4}$//' <<<"$ident")
  if [ "$resolved" != "$want" ]; then
    echo "pre-commit: $which identity '$resolved' != '$want'" >&2
    bad=1
  fi
done
if [ "$bad" = 1 ]; then
  echo "fix: git -c user.name=Eternal130 -c user.email=Eternal130@outlook.com commit ..." >&2
  exit 1
fi
exit 0
