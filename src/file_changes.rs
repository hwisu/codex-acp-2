use std::path::{Path, PathBuf};

use agent_client_protocol::schema::{Diff, ToolCallContent, ToolCallLocation};
use codex_protocol::protocol::FileChange;
use diffy::patch_set::{FileOperation, ParseOptions, PatchKind, PatchSet};
use itertools::Itertools;

use crate::display::tool_call_text_content;

#[derive(Clone, Copy)]
pub(crate) enum FileChangeRenderContext {
    BeforeApply,
    AfterApply,
}

pub(crate) fn extract_tool_call_content_from_changes(
    changes: std::collections::HashMap<PathBuf, FileChange>,
    context: FileChangeRenderContext,
) -> (
    String,
    Vec<ToolCallLocation>,
    impl Iterator<Item = ToolCallContent>,
) {
    let changes = changes
        .into_iter()
        .sorted_by(|a, b| a.0.cmp(&b.0))
        .collect_vec();
    let title = if changes.is_empty() {
        "Edit".to_string()
    } else {
        format!(
            "Edit {}",
            changes
                .iter()
                .map(|(path, change)| tool_call_location_for_change(path, change)
                    .display()
                    .to_string())
                .join(", ")
        )
    };
    let locations = changes
        .iter()
        .map(|(path, change)| ToolCallLocation::new(tool_call_location_for_change(path, change)))
        .collect_vec();
    let content = changes.into_iter().flat_map(move |(path, change)| {
        extract_tool_call_content_from_change(path, change, context)
    });

    (title, locations, content)
}

pub(crate) fn extract_tool_call_content_from_command_output_diff(
    cwd: &Path,
    output: &str,
) -> Option<Vec<ToolCallContent>> {
    extract_tool_call_content_from_git_diff(cwd, output)
        .or_else(|| extract_tool_call_content_from_rustfmt_diff(cwd, output))
}

fn extract_tool_call_content_from_git_diff(
    cwd: &Path,
    output: &str,
) -> Option<Vec<ToolCallContent>> {
    if !output.lines().any(|line| line.starts_with("diff --git ")) {
        return None;
    }

    let patches = PatchSet::parse(output, ParseOptions::gitdiff())
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let content = patches
        .iter()
        .map(|patch| diff_content_from_file_patch(cwd, patch).map(ToolCallContent::Diff))
        .collect::<Option<Vec<_>>>()?;

    (!content.is_empty()).then_some(content)
}

fn diff_content_from_file_patch(
    cwd: &Path,
    file_patch: &diffy::patch_set::FilePatch<'_, str>,
) -> Option<Diff> {
    let PatchKind::Text(patch) = file_patch.patch() else {
        return None;
    };
    let operation = file_patch.operation().strip_prefix(1);

    match operation {
        FileOperation::Create(path) => {
            let path = resolve_command_diff_path(cwd, path.as_ref());
            let new_text = diffy::apply("", patch).ok()?;
            Some(Diff::new(path, new_text))
        }
        FileOperation::Delete(path) => {
            let path = resolve_command_diff_path(cwd, path.as_ref());
            let old_text = std::fs::read_to_string(&path)
                .ok()
                .or_else(|| diffy::apply("", &patch.reverse()).ok())?;
            Some(Diff::new(path, String::new()).old_text(old_text))
        }
        FileOperation::Modify { modified, .. } | FileOperation::Rename { to: modified, .. } => {
            let path = resolve_command_diff_path(cwd, modified.as_ref());
            let new_text = std::fs::read_to_string(&path).ok()?;
            let old_text = diffy::apply(&new_text, &patch.reverse()).ok()?;
            Some(Diff::new(path, new_text).old_text(old_text))
        }
        FileOperation::Copy { .. } => None,
    }
}

fn extract_tool_call_content_from_rustfmt_diff(
    cwd: &Path,
    output: &str,
) -> Option<Vec<ToolCallContent>> {
    let sections = rustfmt_diff_sections(output);
    if sections.is_empty() {
        return None;
    }

    let content = sections
        .into_iter()
        .map(|(path, patch_text)| {
            let path = resolve_command_diff_path(cwd, path);
            let old_text = std::fs::read_to_string(&path).ok()?;
            let patch = diffy::Patch::from_str(&patch_text).ok()?;
            let new_text = diffy::apply(&old_text, &patch).ok()?;
            Some(ToolCallContent::Diff(
                Diff::new(path, new_text).old_text(old_text),
            ))
        })
        .collect::<Option<Vec<_>>>()?;

    (!content.is_empty()).then_some(content)
}

fn rustfmt_diff_sections(output: &str) -> Vec<(PathBuf, String)> {
    let mut sections = Vec::new();
    let mut current: Option<(PathBuf, String)> = None;

    for line in output.lines() {
        if let Some(path) = rustfmt_diff_path_from_header(line) {
            if let Some((path, patch_text)) = current.take()
                && !patch_text.trim().is_empty()
            {
                sections.push((path, patch_text));
            }
            current = Some((path, String::new()));
        } else if let Some((_, patch_text)) = current.as_mut() {
            patch_text.push_str(line);
            patch_text.push('\n');
        }
    }

    if let Some((path, patch_text)) = current
        && !patch_text.trim().is_empty()
    {
        sections.push((path, patch_text));
    }

    sections
}

fn rustfmt_diff_path_from_header(line: &str) -> Option<PathBuf> {
    let rest = line.strip_prefix("Diff in ")?.strip_suffix(':')?;
    let (path, line_number) = rest.rsplit_once(':')?;
    line_number.parse::<usize>().ok()?;
    Some(PathBuf::from(path))
}

fn resolve_command_diff_path(cwd: &Path, path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

fn tool_call_location_for_change(path: &Path, change: &FileChange) -> PathBuf {
    match change {
        FileChange::Update {
            move_path: Some(move_path),
            ..
        } => move_path.clone(),
        _ => path.to_path_buf(),
    }
}

fn extract_tool_call_content_from_change(
    path: PathBuf,
    change: FileChange,
    context: FileChangeRenderContext,
) -> Vec<ToolCallContent> {
    match change {
        FileChange::Add { content } => vec![ToolCallContent::Diff(Diff::new(path, content))],
        FileChange::Delete { content } => {
            let old_text = if content.is_empty() {
                std::fs::read_to_string(&path).unwrap_or(content)
            } else {
                content
            };
            vec![ToolCallContent::Diff(
                Diff::new(path, String::new()).old_text(old_text),
            )]
        }
        FileChange::Update {
            unified_diff,
            move_path,
        } => extract_tool_call_content_from_unified_diff(
            &path,
            move_path.as_deref().unwrap_or(&path),
            unified_diff,
            context,
        ),
    }
}

fn extract_tool_call_content_from_unified_diff(
    source_path: &Path,
    display_path: &Path,
    unified_diff: String,
    context: FileChangeRenderContext,
) -> Vec<ToolCallContent> {
    let Ok(patch) = diffy::Patch::from_str(&unified_diff) else {
        return vec![tool_call_text_content(unified_diff)];
    };

    if let Some(diff) = full_file_diff_from_patch(source_path, display_path, &patch, context) {
        return vec![ToolCallContent::Diff(diff)];
    }

    // ACP Diff content is an old/new file snapshot. If we cannot produce a real
    // snapshot, show the unified diff verbatim instead of turning a hunk into a
    // misleading partial-file snapshot.
    vec![tool_call_text_content(unified_diff)]
}

fn full_file_diff_from_patch(
    source_path: &Path,
    display_path: &Path,
    patch: &diffy::Patch<'_, str>,
    context: FileChangeRenderContext,
) -> Option<Diff> {
    match context {
        FileChangeRenderContext::BeforeApply => {
            let old_text = std::fs::read_to_string(source_path).ok()?;
            let new_text = diffy::apply(&old_text, patch).ok()?;
            Some(Diff::new(display_path, new_text).old_text(old_text))
        }
        FileChangeRenderContext::AfterApply => {
            let new_text = std::fs::read_to_string(display_path).ok()?;
            let reverse_patch = patch.reverse();
            let old_text = diffy::apply(&new_text, &reverse_patch).ok()?;
            Some(Diff::new(display_path, new_text).old_text(old_text))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, fs};

    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join("codex-acp-file-changes-tests")
            .join(uuid::Uuid::new_v4().to_string())
            .join(name)
    }

    fn update_change(unified_diff: &str) -> FileChange {
        FileChange::Update {
            unified_diff: unified_diff.to_string(),
            move_path: None,
        }
    }

    fn first_diff(content: Vec<ToolCallContent>) -> Diff {
        match content.into_iter().next() {
            Some(ToolCallContent::Diff(diff)) => diff,
            other => panic!("expected diff content, got {other:?}"),
        }
    }

    #[test]
    fn update_change_before_apply_uses_full_file_snapshot() {
        let path = temp_path("multi.txt");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "foo\nbar\nbaz\nqux\n").unwrap();
        let unified_diff = "\
@@ -1,4 +1,4 @@
 foo
-bar
+BAR
 baz
-qux
+QUX
";
        let changes = HashMap::from([(path.clone(), update_change(unified_diff))]);

        let (_, _, content) =
            extract_tool_call_content_from_changes(changes, FileChangeRenderContext::BeforeApply);
        let diff = first_diff(content.collect_vec());

        assert_eq!(diff.path, path);
        assert_eq!(diff.old_text.as_deref(), Some("foo\nbar\nbaz\nqux\n"));
        assert_eq!(diff.new_text, "foo\nBAR\nbaz\nQUX\n");
    }

    #[test]
    fn git_diff_command_output_uses_diff_content() {
        let cwd = temp_path("git-diff-root");
        let path = cwd.join("src/lib.rs");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "fn main() { println!(\"hi\"); }\n").unwrap();
        let output = "\
diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1 +1 @@
-fn main(){println!(\"hi\");}
+fn main() { println!(\"hi\"); }
";

        let content =
            extract_tool_call_content_from_command_output_diff(&cwd, output).expect("diff content");
        let diff = first_diff(content);

        assert_eq!(diff.path, path);
        assert_eq!(
            diff.old_text.as_deref(),
            Some("fn main(){println!(\"hi\");}\n")
        );
        assert_eq!(diff.new_text, "fn main() { println!(\"hi\"); }\n");
    }

    #[test]
    fn rustfmt_command_output_uses_diff_content() {
        let cwd = temp_path("rustfmt-root");
        let path = cwd.join("src/main.rs");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "fn main(){println!(\"hi\");}\n").unwrap();
        let output = format!(
            "\
Diff in {}:1:
@@ -1 +1 @@
-fn main(){{println!(\"hi\");}}
+fn main() {{ println!(\"hi\"); }}
",
            path.display()
        );

        let content = extract_tool_call_content_from_command_output_diff(&cwd, &output)
            .expect("diff content");
        let diff = first_diff(content);

        assert_eq!(diff.path, path);
        assert_eq!(
            diff.old_text.as_deref(),
            Some("fn main(){println!(\"hi\");}\n")
        );
        assert_eq!(diff.new_text, "fn main() { println!(\"hi\"); }\n");
    }

    #[test]
    fn update_change_after_apply_uses_full_file_snapshot() {
        let path = temp_path("multi.txt");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "foo\nBAR\nbaz\nQUX\n").unwrap();
        let unified_diff = "\
@@ -1,4 +1,4 @@
 foo
-bar
+BAR
 baz
-qux
+QUX
";
        let changes = HashMap::from([(path.clone(), update_change(unified_diff))]);

        let (_, _, content) =
            extract_tool_call_content_from_changes(changes, FileChangeRenderContext::AfterApply);
        let diff = first_diff(content.collect_vec());

        assert_eq!(diff.path, path);
        assert_eq!(diff.old_text.as_deref(), Some("foo\nbar\nbaz\nqux\n"));
        assert_eq!(diff.new_text, "foo\nBAR\nbaz\nQUX\n");
    }

    #[test]
    fn update_change_falls_back_to_verbatim_diff_when_snapshot_is_unavailable() {
        let path = temp_path("missing.txt");
        let unified_diff = "\
@@ -1 +1 @@
-old
+new
";
        let changes = HashMap::from([(path, update_change(unified_diff))]);

        let (_, _, content) =
            extract_tool_call_content_from_changes(changes, FileChangeRenderContext::BeforeApply);
        let content = content.collect_vec();

        assert!(matches!(
            content.first(),
            Some(ToolCallContent::Content(content))
                if matches!(&content.content, agent_client_protocol::schema::ContentBlock::Text(text) if text.text == unified_diff)
        ));
    }

    #[test]
    fn delete_change_with_empty_content_reads_existing_file_snapshot() {
        let path = temp_path("delete.txt");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "delete me\n").unwrap();
        let changes = HashMap::from([(
            path.clone(),
            FileChange::Delete {
                content: String::new(),
            },
        )]);

        let (_, _, content) =
            extract_tool_call_content_from_changes(changes, FileChangeRenderContext::BeforeApply);
        let diff = first_diff(content.collect_vec());

        assert_eq!(diff.path, path);
        assert_eq!(diff.old_text.as_deref(), Some("delete me\n"));
        assert_eq!(diff.new_text, "");
    }

    #[test]
    fn change_titles_and_locations_are_sorted_by_path() {
        let changes = HashMap::from([
            (
                PathBuf::from("z.txt"),
                FileChange::Add {
                    content: "z\n".to_string(),
                },
            ),
            (
                PathBuf::from("a.txt"),
                FileChange::Add {
                    content: "a\n".to_string(),
                },
            ),
        ]);

        let (title, locations, _) =
            extract_tool_call_content_from_changes(changes, FileChangeRenderContext::BeforeApply);

        assert_eq!(title, "Edit a.txt, z.txt");
        assert_eq!(locations[0].path, PathBuf::from("a.txt"));
        assert_eq!(locations[1].path, PathBuf::from("z.txt"));
    }
}
