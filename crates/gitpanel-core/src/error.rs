use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ValidationError {
    #[error("repository name is empty")]
    EmptyName,
    #[error("repository name is longer than 100 characters")]
    NameTooLong,
    #[error("repository name contains an invalid character")]
    InvalidNameCharacter,
    #[error("description is longer than 350 characters")]
    DescriptionTooLong,
}

impl ValidationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::EmptyName => "validation-name-empty",
            Self::NameTooLong => "validation-name-too-long",
            Self::InvalidNameCharacter => "validation-name-invalid",
            Self::DescriptionTooLong => "validation-description-too-long",
        }
    }
}
