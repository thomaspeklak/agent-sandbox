/// Image-wide pnpm selection: npm's latest stable release or an exact version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PnpmVersion(String);

impl Default for PnpmVersion {
    fn default() -> Self {
        Self("latest".to_owned())
    }
}

impl PnpmVersion {
    pub fn parse(value: &str) -> Result<Self, String> {
        let parts: Vec<_> = value.split('.').collect();
        let exact = parts.len() == 3
            && parts.iter().all(|part| {
                !part.is_empty()
                    && (part.len() == 1 || !part.starts_with('0'))
                    && part.bytes().all(|byte| byte.is_ascii_digit())
                    && part.parse::<u64>().is_ok()
            });
        if value != "latest" && !exact {
            return Err(
                "must be 'latest' or an exact stable version (MAJOR.MINOR.PATCH)".to_owned(),
            );
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
