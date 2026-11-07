use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// A set of accepted blacklist checksums.
#[derive(Debug)]
pub struct Blacklist {
    checksums: BTreeSet<MutationChecksum>,
}

impl Blacklist {
    /// Load checksums from files. Remove trailing carriage returns from each
    /// line and ignore empty lines. Each other line must be exactly 32
    /// lower-case ASCII hexadecimal characters.
    pub fn load(files: &[PathBuf]) -> Result<Self, String> {
        let mut checksums = BTreeSet::new();
        for path in files {
            read_checksums(path, &mut checksums)?;
        }
        Ok(Self { checksums })
    }

    /// Return whether the blacklist contains the checksum.
    pub fn contains(&self, checksum: &MutationChecksum) -> bool {
        self.checksums.contains(checksum)
    }
}

/// A blacklist checksum in 32-character lower-case ASCII hexadecimal form.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MutationChecksum(String);

impl MutationChecksum {
    pub(crate) fn from_changed_lines(lines: String) -> Self {
        Self(format!("{:x}", md5::compute(lines)))
    }

    /// Return the checksum as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn into_string(self) -> String {
        self.0
    }

    /// Parse exactly 32 lower-case ASCII hexadecimal characters.
    pub fn parse(value: &str) -> Option<Self> {
        crate::evidence::valid_hex(value).then(|| Self(value.to_owned()))
    }
}

fn read_checksums(path: &Path, checksums: &mut BTreeSet<MutationChecksum>) -> Result<(), String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("could not read blacklist {}: {error}", path.display()))?;
    for (index, line) in text.lines().enumerate() {
        let checksum = line.trim_end_matches('\r');
        if checksum.is_empty() {
            continue;
        }
        let checksum = MutationChecksum::parse(checksum).ok_or_else(|| {
            format!(
                "blacklist {} line {} must be a 32-character lower-case hexadecimal checksum",
                path.display(),
                index + 1
            )
        })?;
        checksums.insert(checksum);
    }
    Ok(())
}
