//! Port of `tool/plugin/file-diff.ts` + the `diff` npm package's
//! `createTwoFilesPatch`/`diffLines` (verified against the reference
//! package): header block `Index:`/`===…`/`---`/`+++`, unified hunks with a
//! 4-line context, merged when contexts touch, `\ No newline at end of
//! file` markers, and `,1` counts kept explicit (`@@ -1,1 +1,1 @@`).

use oc_schema::session_support::{FileDiff, FileDiffStatus};

/// Context lines around each change (the `diff` package default).
pub const DEFAULT_CONTEXT: usize = 4;

/// The unified-diff context git-side callers pass (`git diff
/// --unified=${context ?? 3}`, ref git.ts:535; the instruction-discovery
/// diff renderer uses `{ context: 3 }`, instruction-discovery.ts:135).
pub const GIT_DEFAULT_CONTEXT: usize = 3;

/// One tokenized line: the text without its terminator plus whether the
/// terminator was a newline (`\r` stripped, like `diffLines` + the package's
/// newline handling on `\r\n` inputs).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Line {
    text: String,
    newline: bool,
}

fn tokenize(text: &str) -> Vec<Line> {
    if text.is_empty() {
        return Vec::new();
    }
    let terminated = text.ends_with('\n');
    let mut parts: Vec<&str> = text.split('\n').collect();
    if terminated {
        parts.pop();
    }
    let last = parts.len() - 1;
    parts
        .into_iter()
        .enumerate()
        .map(|(index, part)| Line {
            text: part.strip_suffix('\r').unwrap_or(part).to_string(),
            newline: index < last || (index == last && terminated),
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Context,
    Delete,
    Insert,
}

/// Annotated op: the op plus its 0-based position in the OLD and NEW
/// arrays (the position BEFORE the op applies).
#[derive(Debug, Clone, Copy)]
struct Annotated {
    op: Op,
    old_pos: usize,
    new_pos: usize,
}

/// Common prefix/suffix trim + LCS over the middle. Equivalent to the
/// package's output for every edit shape the tools produce.
fn diff_ops(old: &[Line], new: &[Line]) -> Vec<Annotated> {
    let mut prefix = 0;
    while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < old.len() - prefix
        && suffix < new.len() - prefix
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let old_mid = &old[prefix..old.len() - suffix];
    let new_mid = &new[prefix..new.len() - suffix];

    let mut middle: Vec<Op> = Vec::new();
    if old_mid.len() * new_mid.len() <= 4_000_000 {
        let mut table = vec![0u32; (old_mid.len() + 1) * (new_mid.len() + 1)];
        let width = new_mid.len() + 1;
        for i in (0..old_mid.len()).rev() {
            for j in (0..new_mid.len()).rev() {
                table[i * width + j] = if old_mid[i] == new_mid[j] {
                    table[(i + 1) * width + j + 1] + 1
                } else {
                    table[(i + 1) * width + j].max(table[i * width + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < old_mid.len() && j < new_mid.len() {
            if old_mid[i] == new_mid[j] {
                middle.push(Op::Context);
                i += 1;
                j += 1;
            } else if table[(i + 1) * width + j] >= table[i * width + j + 1] {
                middle.push(Op::Delete);
                i += 1;
            } else {
                middle.push(Op::Insert);
                j += 1;
            }
        }
        middle.extend(std::iter::repeat_n(Op::Delete, old_mid.len() - i));
        middle.extend(std::iter::repeat_n(Op::Insert, new_mid.len() - j));
    } else {
        // Pathologically large middle: a whole-block replacement is still a
        // valid (just larger) diff.
        middle.extend(std::iter::repeat_n(Op::Delete, old_mid.len()));
        middle.extend(std::iter::repeat_n(Op::Insert, new_mid.len()));
    }

    let mut ops: Vec<Annotated> =
        (0..prefix).map(|i| Annotated { op: Op::Context, old_pos: i, new_pos: i }).collect();
    let (mut old_pos, mut new_pos) = (prefix, prefix);
    for op in middle {
        ops.push(Annotated { op, old_pos, new_pos });
        match op {
            Op::Context => {
                old_pos += 1;
                new_pos += 1;
            }
            Op::Delete => old_pos += 1,
            Op::Insert => new_pos += 1,
        }
    }
    ops.extend((0..suffix).map(|k| Annotated {
        op: Op::Context,
        old_pos: old.len() - suffix + k,
        new_pos: new.len() - suffix + k,
    }));
    ops
}

/// `createTwoFilesPatch(file, file, before, after)` — the package's
/// default 4-line context.
#[must_use]
pub fn two_files_patch(file: &str, before: &str, after: &str) -> String {
    two_files_patch_ctx(file, before, after, DEFAULT_CONTEXT)
}

/// `createTwoFilesPatch(…, { context })` — hunk width under explicit
/// control (jsdiff's `context` option).
#[must_use]
pub fn two_files_patch_ctx(file: &str, before: &str, after: &str, context: usize) -> String {
    let mut out = format!("Index: {file}\n===================================================================\n--- {file}\n+++ {file}\n");
    if before == after {
        return out;
    }
    let old = tokenize(before);
    let new = tokenize(after);
    let ops = diff_ops(&old, &new);

    let mut index = 0;
    while index < ops.len() {
        if ops[index].op == Op::Context {
            index += 1;
            continue;
        }
        // The change run starts here; extend through context runs that are
        // within 2*context of the last change (verified merge/split rule).
        let hunk_start = index.saturating_sub(context);
        let mut changes_end = index;
        let mut group_end = index;
        'extend: while group_end < ops.len() {
            if ops[group_end].op != Op::Context {
                changes_end = group_end;
                group_end += 1;
                continue;
            }
            let mut run = group_end;
            while run < ops.len() && ops[run].op == Op::Context {
                run += 1;
            }
            if run < ops.len() && run - changes_end - 1 <= 2 * context {
                group_end = run;
                continue 'extend;
            }
            group_end = (changes_end + 1 + context).min(run);
            break;
        }
        let hunk = &ops[hunk_start..group_end];
        let old_count = hunk.iter().filter(|a| a.op != Op::Insert).count();
        let new_count = hunk.iter().filter(|a| a.op != Op::Delete).count();
        let first = hunk[0];
        // Unified convention (verified against the package): a zero count
        // side anchors at the position itself, otherwise position + 1.
        let old_start = if old_count == 0 { first.old_pos } else { first.old_pos + 1 };
        let new_start = if new_count == 0 { first.new_pos } else { first.new_pos + 1 };
        out.push_str(&format!("@@ -{old_start},{old_count} +{new_start},{new_count} @@\n"));
        for annotated in hunk {
            let (prefix_char, line) = match annotated.op {
                Op::Context => (' ', &old[annotated.old_pos]),
                Op::Delete => ('-', &old[annotated.old_pos]),
                Op::Insert => ('+', &new[annotated.new_pos]),
            };
            out.push(prefix_char);
            out.push_str(&line.text);
            out.push('\n');
            if !line.newline {
                out.push_str("\\ No newline at end of file\n");
            }
        }
        index = group_end;
    }
    out
}

/// `fileDiff(file, before, after, status)` — counts from the line diff.
#[must_use]
pub fn file_diff(file: &str, before: &str, after: &str, status: FileDiffStatus) -> FileDiff {
    file_diff_ctx(file, before, after, status, DEFAULT_CONTEXT)
}

/// `fileDiff(…)` with an explicit diff context (the git tree-diff callers:
/// `--unified=${context ?? 3}`).
#[must_use]
pub fn file_diff_ctx(
    file: &str,
    before: &str,
    after: &str,
    status: FileDiffStatus,
    context: usize,
) -> FileDiff {
    let ops = diff_ops(&tokenize(before), &tokenize(after));
    let additions: u32 = ops.iter().filter(|a| a.op == Op::Insert).count() as u32;
    let deletions: u32 = ops.iter().filter(|a| a.op == Op::Delete).count() as u32;
    FileDiff {
        file: file.to_string(),
        patch: two_files_patch_ctx(file, before, after, context),
        additions: additions.into(),
        deletions: deletions.into(),
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_diff_matches_the_reference_package_output() {
        // Verified byte-for-byte against the `diff` npm package used by v2.
        assert_eq!(
            two_files_patch("f", "before\nrest\n", "after\nrest\n"),
            "Index: f\n===================================================================\n--- f\n+++ f\n@@ -1,2 +1,2 @@\n-before\n+after\n rest\n"
        );
        assert_eq!(
            two_files_patch("f", "a", "b"),
            "Index: f\n===================================================================\n--- f\n+++ f\n@@ -1,1 +1,1 @@\n-a\n\\ No newline at end of file\n+b\n\\ No newline at end of file\n"
        );
        assert_eq!(
            two_files_patch("f", "a\nb", "a\nc"),
            "Index: f\n===================================================================\n--- f\n+++ f\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+c\n\\ No newline at end of file\n"
        );
        assert_eq!(
            two_files_patch("f", "", "x\ny\n"),
            "Index: f\n===================================================================\n--- f\n+++ f\n@@ -0,0 +1,2 @@\n+x\n+y\n"
        );
        assert_eq!(
            two_files_patch("f", "x\ny\n", ""),
            "Index: f\n===================================================================\n--- f\n+++ f\n@@ -1,2 +0,0 @@\n-x\n-y\n"
        );
        assert_eq!(
            two_files_patch("f", "", ""),
            "Index: f\n===================================================================\n--- f\n+++ f\n"
        );
        assert_eq!(
            two_files_patch("f", "a\n", "a\n"),
            "Index: f\n===================================================================\n--- f\n+++ f\n"
        );
    }

    #[test]
    fn file_diff_context_four_merges_touching_groups_and_splits_clean_gaps() {
        let old: String = (1..=15).map(|n| format!("{n}\n")).collect();
        let close: String = (1..=15)
            .map(|n| match n {
                5 => "X\n".to_string(),
                14 => "Y\n".to_string(),
                _ => format!("{n}\n"),
            })
            .collect();
        assert_eq!(
            two_files_patch("f", &old, &close),
            "Index: f\n===================================================================\n--- f\n+++ f\n@@ -1,15 +1,15 @@\n 1\n 2\n 3\n 4\n-5\n+X\n 6\n 7\n 8\n 9\n 10\n 11\n 12\n 13\n-14\n+Y\n 15\n"
        );
        let far: String = (1..=15)
            .map(|n| match n {
                5 => "X\n".to_string(),
                15 => "Y\n".to_string(),
                _ => format!("{n}\n"),
            })
            .collect();
        assert_eq!(
            two_files_patch("f", &old, &far),
            "Index: f\n===================================================================\n--- f\n+++ f\n@@ -1,9 +1,9 @@\n 1\n 2\n 3\n 4\n-5\n+X\n 6\n 7\n 8\n 9\n@@ -11,5 +11,5 @@\n 11\n 12\n 13\n 14\n-15\n+Y\n"
        );
    }

    #[test]
    fn file_diff_explicit_context_narrows_hunks() {
        // jsdiff `{ context: 1 }`: one context line around the change
        // instead of the default four, and a smaller gap splits hunks.
        let old: String = (1..=10).map(|n| format!("{n}\n")).collect();
        let new: String = (1..=10)
            .map(|n| match n {
                2 => "X\n".to_string(),
                8 => "Y\n".to_string(),
                _ => format!("{n}\n"),
            })
            .collect();
        assert_eq!(
            two_files_patch_ctx("f", &old, &new, 1),
            "Index: f\n===================================================================\n--- f\n+++ f\n@@ -1,3 +1,3 @@\n 1\n-2\n+X\n 3\n@@ -7,3 +7,3 @@\n 7\n-8\n+Y\n 9\n"
        );
        // context 0: bare change lines, zero-count sides anchor at the
        // change position itself.
        let single = two_files_patch_ctx("f", "a\nb\nc\n", "a\nB\nc\n", 0);
        assert_eq!(
            single,
            "Index: f\n===================================================================\n--- f\n+++ f\n@@ -2,1 +2,1 @@\n-b\n+B\n"
        );
    }

    #[test]
    fn file_diff_counts_additions_and_deletions() {
        let diff =
            file_diff("hello.txt", "before\nrest\n", "after\nrest\n", FileDiffStatus::Modified);
        assert_eq!(diff.additions, 1);
        assert_eq!(diff.deletions, 1);
        assert_eq!(diff.status, FileDiffStatus::Modified);
        assert!(diff.patch.contains("-before\n+after"));
        let added = file_diff("new.txt", "", "created\n", FileDiffStatus::Added);
        assert_eq!(added.additions, 1);
        assert_eq!(added.deletions, 0);
        assert!(added.patch.contains("+created"));
    }
}
