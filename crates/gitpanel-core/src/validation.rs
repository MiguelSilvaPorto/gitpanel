use crate::ValidationError;

/// GitHub repository names allow letters, digits, hyphens, underscores and dots.
/// `.` and `..` are explicitly rejected.
pub fn validate_repo_name(name: &str) -> Result<(), ValidationError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ValidationError::EmptyName);
    }
    if trimmed.chars().count() > 100 {
        return Err(ValidationError::NameTooLong);
    }
    if trimmed == "." || trimmed == ".." {
        return Err(ValidationError::InvalidNameCharacter);
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(ValidationError::InvalidNameCharacter);
    }
    Ok(())
}

pub fn validate_description(description: Option<&str>) -> Result<(), ValidationError> {
    if description.is_some_and(|value| value.chars().count() > 350) {
        return Err(ValidationError::DescriptionTooLong);
    }
    Ok(())
}

/// Destructive actions must be confirmed with the full `owner/repo` name.
pub fn confirmation_matches(full_name: &str, typed: &str) -> bool {
    full_name.trim().eq_ignore_ascii_case(typed.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_names() {
        for name in ["gitpanel", "my.repo", "a_b-c", ".github"] {
            assert!(validate_repo_name(name).is_ok(), "{name}");
        }
    }

    #[test]
    fn rejects_invalid_names() {
        assert_eq!(validate_repo_name(""), Err(ValidationError::EmptyName));
        assert_eq!(
            validate_repo_name("."),
            Err(ValidationError::InvalidNameCharacter)
        );
        assert_eq!(
            validate_repo_name("repo com espaços"),
            Err(ValidationError::InvalidNameCharacter)
        );
        assert_eq!(
            validate_repo_name(&"a".repeat(101)),
            Err(ValidationError::NameTooLong)
        );
    }

    #[test]
    fn description_limit_is_checked() {
        assert!(validate_description(Some(&"a".repeat(350))).is_ok());
        assert_eq!(
            validate_description(Some(&"a".repeat(351))),
            Err(ValidationError::DescriptionTooLong)
        );
    }

    #[test]
    fn confirmation_is_case_insensitive_but_full() {
        assert!(confirmation_matches(
            "MiguelSilvaPorto/gitpanel",
            "miguelsilvaporto/GITPANEL"
        ));
        assert!(!confirmation_matches(
            "MiguelSilvaPorto/gitpanel",
            "gitpanel"
        ));
    }
}
