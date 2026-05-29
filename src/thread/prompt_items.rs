use std::path::Path;

use agent_client_protocol::schema::{
    BlobResourceContents, ContentBlock, EmbeddedResource, EmbeddedResourceResource, ResourceLink,
    TextResourceContents,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use codex_protocol::user_input::UserInput;

pub(super) fn build_prompt_items(
    prompt: Vec<ContentBlock>,
    file_resource_root: Option<&Path>,
) -> Vec<UserInput> {
    let file_resource_root = file_resource_root.and_then(|root| root.canonicalize().ok());
    prompt
        .into_iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text_block) => Some(UserInput::Text {
                text: text_block.text,
                text_elements: vec![],
            }),
            ContentBlock::Image(image_block) => Some(UserInput::Image {
                image_url: format!("data:{};base64,{}", image_block.mime_type, image_block.data),
                detail: None,
            }),
            ContentBlock::ResourceLink(ResourceLink { name, uri, .. }) => Some(UserInput::Text {
                text: format_uri_as_link(Some(name), uri),
                text_elements: vec![],
            }),
            ContentBlock::Resource(EmbeddedResource {
                resource:
                    EmbeddedResourceResource::TextResourceContents(TextResourceContents {
                        text,
                        mime_type,
                        uri,
                        ..
                    }),
                ..
            }) => Some(text_resource_to_user_input(
                text,
                mime_type,
                uri,
                file_resource_root.as_deref(),
            )),
            ContentBlock::Resource(EmbeddedResource {
                resource:
                    EmbeddedResourceResource::BlobResourceContents(BlobResourceContents {
                        blob,
                        mime_type,
                        uri,
                        ..
                    }),
                ..
            }) => Some(blob_resource_to_user_input(blob, mime_type, uri)),
            // Skip other content types for now.
            ContentBlock::Audio(..) | ContentBlock::Resource(..) | _ => None,
        })
        .collect()
}

fn text_resource_to_user_input(
    text: String,
    mime_type: Option<String>,
    uri: String,
    file_resource_root: Option<&Path>,
) -> UserInput {
    if let Some(mime_type) = mime_type.as_deref()
        && is_image_mime_type(mime_type)
    {
        if let Some(image_url) = file_uri_to_image_data_url(&uri, mime_type, file_resource_root) {
            return UserInput::Image {
                image_url,
                detail: None,
            };
        }

        return omitted_resource_marker(uri, Some(mime_type), "Image resource omitted");
    }

    if text_looks_binary(&text) {
        return omitted_resource_marker(
            uri,
            mime_type.as_deref(),
            "Binary-looking text resource omitted",
        );
    }

    UserInput::Text {
        text: format!(
            "{}\n<context ref=\"{uri}\">\n{text}\n</context>",
            format_uri_as_link(None, uri.clone())
        ),
        text_elements: vec![],
    }
}

fn blob_resource_to_user_input(blob: String, mime_type: Option<String>, uri: String) -> UserInput {
    if let Some(mime_type) = mime_type.as_deref()
        && is_image_mime_type(mime_type)
    {
        return UserInput::Image {
            image_url: format!("data:{mime_type};base64,{blob}"),
            detail: None,
        };
    }

    let mime_type = mime_type
        .filter(|mime_type| !mime_type.trim().is_empty())
        .unwrap_or_else(|| "unknown MIME type".to_string());
    UserInput::Text {
        text: format!(
            "{}\n[Binary resource omitted: {mime_type}]",
            format_uri_as_link(None, uri)
        ),
        text_elements: vec![],
    }
}

fn omitted_resource_marker(uri: String, mime_type: Option<&str>, reason: &str) -> UserInput {
    let mime_type = mime_type
        .filter(|mime_type| !mime_type.trim().is_empty())
        .unwrap_or("unknown MIME type");
    UserInput::Text {
        text: format!("{}\n[{reason}: {mime_type}]", format_uri_as_link(None, uri)),
        text_elements: vec![],
    }
}

fn is_image_mime_type(mime_type: &str) -> bool {
    mime_type.to_ascii_lowercase().starts_with("image/")
}

fn file_uri_to_image_data_url(
    uri: &str,
    mime_type: &str,
    file_resource_root: Option<&Path>,
) -> Option<String> {
    let file_resource_root = file_resource_root?;
    let path = uri.strip_prefix("file://")?;
    let path = Path::new(path).canonicalize().ok()?;
    if !path.starts_with(file_resource_root) {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(format!(
        "data:{mime_type};base64,{}",
        BASE64_STANDARD.encode(bytes)
    ))
}

fn text_looks_binary(text: &str) -> bool {
    const SAMPLE_LIMIT: usize = 4096;
    let mut total = 0usize;
    let mut suspicious = 0usize;

    for ch in text.chars().take(SAMPLE_LIMIT) {
        total += 1;
        if ch == '\0' || ch == '\u{FFFD}' || (ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
        {
            suspicious += 1;
        }
    }

    total > 0 && suspicious * 20 > total
}

fn format_uri_as_link(name: Option<String>, uri: String) -> String {
    if let Some(name) = name
        && !name.is_empty()
    {
        format!("[@{name}]({uri})")
    } else if let Some(path) = uri.strip_prefix("file://") {
        let name = path.split('/').next_back().unwrap_or(path);
        format!("[@{name}]({uri})")
    } else if uri.starts_with("zed://") {
        let name = uri.split('/').next_back().unwrap_or(&uri);
        format!("[@{name}]({uri})")
    } else {
        uri
    }
}

pub(super) fn replace_first_text_item(items: &mut [UserInput], text: String) {
    if let Some(UserInput::Text { text: existing, .. }) = items.first_mut() {
        *existing = text;
    }
}

/// Checks if a prompt is slash command.
pub(super) fn extract_slash_command(content: &[UserInput]) -> Option<(&str, &str)> {
    let line = content.first().and_then(|block| match block {
        UserInput::Text { text, .. } => Some(text),
        _ => None,
    })?;
    // Parse a first-line slash command of the form `/name <rest>`.
    // Returns `(name, rest_after_name)` if the line begins with `/` and contains
    // a non-empty name; otherwise returns `None`.
    let stripped = line.strip_prefix('/')?;
    let mut name_end = stripped.len();
    for (idx, ch) in stripped.char_indices() {
        if ch.is_whitespace() {
            name_end = idx;
            break;
        }
    }
    let name = &stripped[..name_end];
    if name.is_empty() {
        return None;
    }
    let rest = stripped[name_end..].trim_start();
    Some((name, rest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::{
        BlobResourceContents, EmbeddedResourceResource, TextResourceContents,
    };

    fn blob_resource(blob: &str, uri: &str, mime_type: Option<&str>) -> ContentBlock {
        let mut resource = BlobResourceContents::new(blob, uri);
        if let Some(mime_type) = mime_type {
            resource = resource.mime_type(mime_type);
        }
        ContentBlock::Resource(EmbeddedResource::new(
            EmbeddedResourceResource::BlobResourceContents(resource),
        ))
    }

    fn text_resource(text: &str, uri: &str, mime_type: Option<&str>) -> ContentBlock {
        let mut resource = TextResourceContents::new(text, uri);
        if let Some(mime_type) = mime_type {
            resource = resource.mime_type(mime_type);
        }
        ContentBlock::Resource(EmbeddedResource::new(
            EmbeddedResourceResource::TextResourceContents(resource),
        ))
    }

    #[test]
    fn toad_image_blob_resource_becomes_image_input() {
        let items = build_prompt_items(
            vec![blob_resource(
                "iVBORw0KGgo=",
                "file:///tmp/frog.png",
                Some("image/png"),
            )],
            None,
        );

        assert_eq!(
            items,
            vec![UserInput::Image {
                image_url: "data:image/png;base64,iVBORw0KGgo=".to_string(),
                detail: None,
            }]
        );
    }

    #[test]
    fn non_image_blob_resource_keeps_uri_and_mime_without_blob_body() {
        let items = build_prompt_items(
            vec![blob_resource(
                "YWJjZGVm",
                "file:///tmp/archive.zip",
                Some("application/zip"),
            )],
            None,
        );

        let [UserInput::Text { text, .. }] = items.as_slice() else {
            panic!("non-image blob should become a text marker");
        };
        assert!(text.contains("[@archive.zip](file:///tmp/archive.zip)"));
        assert!(text.contains("application/zip"));
        assert!(!text.contains("YWJjZGVm"));
    }

    #[test]
    fn toad_image_text_resource_reads_file_uri_as_image_input() -> anyhow::Result<()> {
        let dir = std::env::temp_dir().join(format!("codex-acp-image-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir)?;
        let image_path = dir.join("frog.png");
        std::fs::write(&image_path, [0x89, b'P', b'N', b'G'])?;
        let uri = format!("file://{}", image_path.display());

        let items = build_prompt_items(
            vec![text_resource("not the image", &uri, Some("image/png"))],
            Some(&dir),
        );

        assert_eq!(
            items,
            vec![UserInput::Image {
                image_url: "data:image/png;base64,iVBORw==".to_string(),
                detail: None,
            }]
        );

        std::fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[test]
    fn image_text_resource_without_readable_file_omits_text_body() {
        let items = build_prompt_items(
            vec![text_resource(
                "garbled image body",
                "file:///tmp/missing-frog.png",
                Some("image/png"),
            )],
            Some(std::env::temp_dir().as_path()),
        );

        let [UserInput::Text { text, .. }] = items.as_slice() else {
            panic!("unreadable image text resource should become a marker");
        };
        assert!(text.contains("[@missing-frog.png](file:///tmp/missing-frog.png)"));
        assert!(text.contains("Image resource omitted: image/png"));
        assert!(!text.contains("garbled image body"));
    }

    #[test]
    fn image_text_resource_outside_root_omits_text_body() -> anyhow::Result<()> {
        let root = std::env::temp_dir().join(format!("codex-acp-root-{}", uuid::Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("codex-acp-outside-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root)?;
        std::fs::create_dir_all(&outside)?;
        let image_path = outside.join("frog.png");
        std::fs::write(&image_path, [0x89, b'P', b'N', b'G'])?;
        let uri = format!("file://{}", image_path.display());

        let items = build_prompt_items(
            vec![text_resource("garbled image body", &uri, Some("image/png"))],
            Some(&root),
        );

        let [UserInput::Text { text, .. }] = items.as_slice() else {
            panic!("outside-root image text resource should become a marker");
        };
        assert!(text.contains("Image resource omitted: image/png"));
        assert!(!text.contains("garbled image body"));

        std::fs::remove_dir_all(root)?;
        std::fs::remove_dir_all(outside)?;
        Ok(())
    }

    #[test]
    fn binary_looking_text_resource_omits_text_body() {
        let items = build_prompt_items(
            vec![text_resource(
                "\0\0\u{FFFD}\u{FFFD}raw",
                "file:///tmp/archive.bin",
                Some("application/octet-stream"),
            )],
            None,
        );

        let [UserInput::Text { text, .. }] = items.as_slice() else {
            panic!("binary-looking text resource should become a marker");
        };
        assert!(text.contains("[@archive.bin](file:///tmp/archive.bin)"));
        assert!(text.contains("Binary-looking text resource omitted: application/octet-stream"));
        assert!(!text.contains("raw"));
    }

    #[test]
    fn text_embedded_resource_still_becomes_context() {
        let items = build_prompt_items(
            vec![text_resource(
                "hello",
                "file:///tmp/notes.md",
                Some("text/markdown"),
            )],
            None,
        );

        assert_eq!(
            items,
            vec![UserInput::Text {
                text: "[@notes.md](file:///tmp/notes.md)\n<context ref=\"file:///tmp/notes.md\">\nhello\n</context>"
                    .to_string(),
                text_elements: vec![],
            }]
        );
    }
}
