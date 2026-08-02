use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) struct Blacklist {
    checksums: BTreeSet<MutationChecksum>,
}

impl Blacklist {
    pub(crate) fn load(files: &[PathBuf]) -> Result<Self, String> {
        let mut checksums = BTreeSet::new();
        for path in files {
            read_checksums(path, &mut checksums)?;
        }
        Ok(Self { checksums })
    }

    pub(crate) fn contains_or_insert(&mut self, checksum: &MutationChecksum) -> bool {
        !self.checksums.insert(checksum.clone())
    }
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct MutationChecksum(String);

impl MutationChecksum {
    pub(crate) fn from_changed_lines(lines: String) -> Self {
        Self(format!("{:x}", md5::compute(lines)))
    }

    fn parse(value: &str) -> Option<Self> {
        if value.len() == 32
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            Some(Self(value.to_owned()))
        } else {
            None
        }
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
