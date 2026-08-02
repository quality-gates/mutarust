use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::evidence::StableMutantId;
use crate::{MutationRun, MutationState};

const BASELINE_VERSION: u8 = 1;

/// A set of escaped stable mutant IDs that a project accepts.
pub struct Baseline {
    accepted_ids: BTreeSet<String>,
}

impl Baseline {
    /// Reads a baseline. A missing file means that no escaped mutant is accepted.
    pub fn load(path: &Path) -> Result<Self, String> {
        let Some(document) = read_document(path)? else {
            return Ok(Self {
                accepted_ids: BTreeSet::new(),
            });
        };
        validate_version(path, document.version)?;
        let accepted_ids = accepted_ids(path, document.mutants)?;
        Ok(Self { accepted_ids })
    }

    /// Returns the escaped mutants that this baseline does not accept.
    pub fn new_escaped_count(&self, run: &MutationRun) -> usize {
        run.results()
            .iter()
            .filter(|result| result.state == MutationState::Escaped)
            .filter(|result| !self.accepted_ids.contains(&result.stable_id))
            .count()
    }

    /// Writes the current escaped mutants to a baseline file.
    pub fn write(path: &Path, run: &MutationRun) -> Result<usize, String> {
        let mut mutants = BTreeMap::new();
        for result in run
            .results()
            .iter()
            .filter(|result| result.state == MutationState::Escaped)
        {
            mutants
                .entry(result.stable_id.clone())
                .or_insert_with(|| BaselineMutant {
                    id: result.stable_id.clone(),
                    file: portable_path(&result.source),
                    mutator: result.mutator.clone(),
                    line: result.line,
                });
        }
        let document = BaselineDocument {
            version: BASELINE_VERSION,
            mutants: mutants.into_values().collect(),
        };
        let mut text = serde_json::to_string_pretty(&document)
            .map_err(|error| format!("could not write baseline {}: {error}", path.display()))?;
        text.push('\n');
        fs::write(path, text)
            .map_err(|error| format!("could not write baseline {}: {error}", path.display()))?;
        Ok(document.mutants.len())
    }
}

fn read_document(path: &Path) -> Result<Option<BaselineDocument>, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "could not read baseline {}: {error}",
                path.display()
            ));
        }
    };
    serde_json::from_str::<BaselineDocument>(&text)
        .map(Some)
        .map_err(|error| format!("could not parse baseline {}: {error}", path.display()))
}

fn validate_version(path: &Path, version: u8) -> Result<(), String> {
    if version == BASELINE_VERSION {
        Ok(())
    } else {
        Err(format!(
            "baseline {} has unsupported version {version}",
            path.display()
        ))
    }
}

fn accepted_ids(path: &Path, mutants: Vec<BaselineMutant>) -> Result<BTreeSet<String>, String> {
    let mut accepted_ids = BTreeSet::new();
    for mutant in mutants {
        validate_mutant(path, &mutant)?;
        if !accepted_ids.insert(mutant.id.clone()) {
            return Err(format!(
                "baseline {} has duplicate mutant ID {}",
                path.display(),
                mutant.id
            ));
        }
    }
    Ok(accepted_ids)
}

fn validate_mutant(path: &Path, mutant: &BaselineMutant) -> Result<(), String> {
    if StableMutantId::parse(&mutant.id).is_none() {
        return Err(format!(
            "baseline {} has malformed mutant ID {}",
            path.display(),
            mutant.id
        ));
    }
    if mutant.file.is_empty() || mutant.mutator.is_empty() || mutant.line == 0 {
        return Err(format!(
            "baseline {} has incomplete mutant metadata for {}",
            path.display(),
            mutant.id
        ));
    }
    Ok(())
}

#[derive(Deserialize, Serialize)]
struct BaselineDocument {
    version: u8,
    mutants: Vec<BaselineMutant>,
}

#[derive(Deserialize, Serialize)]
struct BaselineMutant {
    id: String,
    file: String,
    mutator: String,
    line: usize,
}

fn portable_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
