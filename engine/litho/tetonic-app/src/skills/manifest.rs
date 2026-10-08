use yaml_rust2::{
    scanner::{Scanner, TokenType},
    YamlLoader,
};

/// Standalone SKILL.md profile. No bundled scripts, downloads, or implicit grants.
pub(crate) fn parse_skill(content: &str) -> Result<(String, String), String> {
    if content.len() > 32768 || content.contains('\0') {
        return Err("SKILL.md must be at most 32 KiB and contain no null bytes.".into());
    }
    let normalized = content.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let Some(rest) = normalized.strip_prefix("---\n") else {
        return Err("Start SKILL.md with YAML frontmatter containing name and description.".into());
    };
    let Some((header, body)) = rest.split_once("\n---\n") else {
        return Err(
            "Close the YAML frontmatter with --- on its own line, then add instructions.".into(),
        );
    };
    if header.len() > 4096 || body.trim().is_empty() {
        return Err("Use at most 4 KiB of frontmatter and include skill instructions.".into());
    }
    // Bound depth and reject aliases before the tree loader can expand anything.
    let mut depth = 0usize;
    for token in Scanner::new(header.chars()) {
        match token.1 {
            TokenType::Anchor(_) | TokenType::Alias(_) | TokenType::Tag(_, _) => {
                return Err("YAML anchors, aliases, and tags are not supported.".into())
            }
            TokenType::FlowMappingStart
            | TokenType::BlockMappingStart
            | TokenType::FlowSequenceStart
            | TokenType::BlockSequenceStart => {
                depth += 1;
                if depth > 8 {
                    return Err("Skill metadata is nested too deeply.".into());
                }
            }
            TokenType::BlockEnd | TokenType::FlowMappingEnd | TokenType::FlowSequenceEnd => {
                depth = depth.saturating_sub(1)
            }
            _ => {}
        }
    }
    let docs =
        YamlLoader::load_from_str(header).map_err(|_| "Invalid YAML frontmatter.".to_string())?;
    if docs.len() != 1 || docs[0].as_hash().is_none() {
        return Err("Use one YAML mapping for skill metadata.".into());
    }
    let name = docs[0]["name"].as_str().unwrap_or_default();
    let description = docs[0]["description"].as_str().unwrap_or_default().trim();
    if name.is_empty()
        || name.len() > 64
        || name.starts_with('-')
        || name.ends_with('-')
        || name.contains("--")
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(
            "Use a skill name of 1–64 lowercase letters, numbers, and single hyphens.".into(),
        );
    }
    if description.is_empty() || description.chars().count() > 1024 {
        return Err(
            "Add a description of 1–1,024 characters explaining when to use this skill.".into(),
        );
    }
    Ok((name.into(), description.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_standard_frontmatter_and_rejects_unbounded_yaml() {
        assert_eq!(parse_skill("---\nname: research\ndescription: >\n  Find reliable\n  evidence.\n---\nCheck sources.").unwrap(), ("research".into(), "Find reliable evidence.".into()));
        for bad in [
            "---\nname: Bad Name\ndescription: x\n---\nx",
            "---\nname: research\ndescription: &a [*a]\n---\nx",
            "---\nname: research\ndescription: x\n---\n",
            "no header",
        ] {
            assert!(parse_skill(bad).is_err());
        }
    }
}
