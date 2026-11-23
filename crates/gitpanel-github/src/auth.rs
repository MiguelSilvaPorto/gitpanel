use std::fmt;
use std::process::Command;

use zeroize::Zeroize;

use crate::error::GithubError;

pub struct Token(String);

impl Token {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn from_gh() -> Result<Self, GithubError> {
        let output = Command::new("gh")
            .args(["auth", "token", "--hostname", "github.com"])
            .output()
            .map_err(|source| {
                if source.kind() == std::io::ErrorKind::NotFound {
                    GithubError::GhMissing
                } else {
                    GithubError::GhCommand(source.to_string())
                }
            })?;
        if !output.status.success() {
            let details = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(GithubError::GhNotLoggedIn { details });
        }
        let token = String::from_utf8(output.stdout)
            .map_err(|_| GithubError::GhNotLoggedIn {
                details: "gh returned a token that is not valid UTF-8".to_owned(),
            })?
            .trim()
            .to_owned();
        if token.is_empty() {
            return Err(GithubError::GhNotLoggedIn {
                details: "gh returned an empty token".to_owned(),
            });
        }
        Ok(Self(token))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Drop for Token {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(***)")
    }
}
