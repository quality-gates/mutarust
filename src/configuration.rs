use std::fmt;
use std::fs;
use std::path::Path;

use regex::Regex;
use serde::Deserialize;

/// The mutation policy read from a Mutarust YAML file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Configuration {
    /// Skips production source files that have no `#[cfg(test)]` unit tests.
    pub skip_without_test: bool,
    /// Skips mutants in production items gated by a non-test `#[cfg(...)]`.
    pub skip_with_cfg: bool,
    /// Requests a JSON report.
    pub json_output: bool,
    /// Requests an HTML report.
    pub html_output: bool,
    /// Hides per-mutant status output.
    pub silent_mode: bool,
    /// The minimum mutation score percentage.
    pub min_msi: Option<u8>,
    /// The minimum covered-code mutation score percentage.
    pub min_covered_msi: Option<u8>,
    /// Source directory prefixes that Mutarust excludes.
    pub exclude_dirs: Vec<String>,
    /// Mutator names or group patterns that Mutarust disables.
    pub disable_mutators: Vec<String>,
    /// Mutator names or group patterns that Mutarust enables.
    pub enable_mutators: Vec<String>,
    /// Regular expressions for ignored source lines.
    pub ignore_source_lines: Vec<String>,
}

impl Configuration {
    /// Reads and validates a YAML configuration file.
    pub fn read(path: &Path) -> Result<Self, ConfigurationError> {
        let text = fs::read_to_string(path).map_err(|error| {
            ConfigurationError::new(format!(
                "could not read configuration {}: {error}",
                path.display()
            ))
        })?;
        let file: FileConfiguration = yaml_serde::from_str(&text).map_err(|error| {
            ConfigurationError::new(format!(
                "could not parse configuration {}: {error}",
                path.display()
            ))
        })?;
        Self::from_file(file).map_err(|error| error.with_path(path))
    }

    /// Applies the command settings that have a supplied value.
    pub fn apply(&mut self, settings: &CommandSettings) -> Result<(), ConfigurationError> {
        apply_boolean(&mut self.silent_mode, settings.silent_mode);
        apply_score(&mut self.min_msi, settings.min_msi);
        apply_score(&mut self.min_covered_msi, settings.min_covered_msi);
        apply_patterns(&mut self.enable_mutators, &settings.enable_mutators);
        self.disable_mutators
            .extend(settings.disable_mutators.iter().cloned());
        self.validate()
    }

    /// Returns the selected mutator names from the supplied known names.
    pub fn select_mutators(
        &self,
        known_names: &[String],
    ) -> Result<Vec<String>, ConfigurationError> {
        validate_matching_patterns("enable_mutators", &self.enable_mutators, known_names)?;
        validate_matching_patterns("disable_mutators", &self.disable_mutators, known_names)?;
        Ok(known_names
            .iter()
            .filter(|name| self.enabled(name))
            .cloned()
            .collect())
    }

    fn from_file(file: FileConfiguration) -> Result<Self, ConfigurationError> {
        let configuration = Self {
            skip_without_test: file.skip_without_test.unwrap_or_default(),
            skip_with_cfg: file.skip_with_cfg.unwrap_or_default(),
            json_output: file.json_output.unwrap_or_default(),
            html_output: file.html_output.unwrap_or_default(),
            silent_mode: file.silent_mode.unwrap_or_default(),
            min_msi: file.min_msi,
            min_covered_msi: file.min_covered_msi,
            exclude_dirs: file.exclude_dirs.unwrap_or_default(),
            disable_mutators: file.disable_mutators.unwrap_or_default(),
            enable_mutators: file.enable_mutators.unwrap_or_default(),
            ignore_source_lines: file.ignore_source_lines.unwrap_or_default(),
        };
        configuration.validate()?;
        Ok(configuration)
    }

    fn validate(&self) -> Result<(), ConfigurationError> {
        validate_score("min_msi", self.min_msi)?;
        validate_score("min_covered_msi", self.min_covered_msi)?;
        validate_directories(&self.exclude_dirs)?;
        validate_mutator_patterns("disable_mutators", &self.disable_mutators)?;
        validate_mutator_patterns("enable_mutators", &self.enable_mutators)?;
        validate_regular_expressions(&self.ignore_source_lines)
    }

    fn enabled(&self, name: &str) -> bool {
        let allowed = self.enable_mutators.is_empty()
            || self
                .enable_mutators
                .iter()
                .any(|pattern| mutator_pattern_matches(pattern, name));
        allowed
            && !self
                .disable_mutators
                .iter()
                .any(|pattern| mutator_pattern_matches(pattern, name))
    }
}

/// Command values that take priority over configuration values when supplied.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommandSettings {
    /// The explicit command setting for status output.
    pub silent_mode: Option<bool>,
    /// The explicit command mutation score gate.
    pub min_msi: Option<u8>,
    /// The explicit command covered-code mutation score gate.
    pub min_covered_msi: Option<u8>,
    /// The explicit command mutator allowlist.
    pub enable_mutators: Option<Vec<String>>,
    /// Additional command mutator denylist patterns.
    pub disable_mutators: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct FileConfiguration {
    skip_without_test: Option<bool>,
    skip_with_cfg: Option<bool>,
    json_output: Option<bool>,
    html_output: Option<bool>,
    silent_mode: Option<bool>,
    min_msi: Option<u8>,
    min_covered_msi: Option<u8>,
    exclude_dirs: Option<Vec<String>>,
    disable_mutators: Option<Vec<String>>,
    enable_mutators: Option<Vec<String>>,
    ignore_source_lines: Option<Vec<String>>,
}

/// A configuration file or value error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigurationError {
    message: String,
}

impl ConfigurationError {
    fn new(message: String) -> Self {
        Self { message }
    }

    fn with_path(mut self, path: &Path) -> Self {
        self.message = format!("configuration {}: {}", path.display(), self.message);
        self
    }
}

impl fmt::Display for ConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ConfigurationError {}

fn validate_score(field: &str, score: Option<u8>) -> Result<(), ConfigurationError> {
    if score.is_some_and(|score| score > 100) {
        return Err(ConfigurationError::new(format!(
            "{field} must be a whole percentage from 0 to 100"
        )));
    }
    Ok(())
}

fn apply_boolean(value: &mut bool, setting: Option<bool>) {
    if let Some(setting) = setting {
        *value = setting;
    }
}

fn apply_score(value: &mut Option<u8>, setting: Option<u8>) {
    if let Some(setting) = setting {
        *value = Some(setting);
    }
}

fn apply_patterns(value: &mut Vec<String>, setting: &Option<Vec<String>>) {
    if let Some(setting) = setting {
        *value = setting.clone();
    }
}

fn validate_directories(directories: &[String]) -> Result<(), ConfigurationError> {
    for (index, directory) in directories.iter().enumerate() {
        if directory.trim().is_empty() {
            return Err(ConfigurationError::new(format!(
                "exclude_dirs[{index}] must not be empty"
            )));
        }
    }
    Ok(())
}

fn validate_mutator_patterns(field: &str, patterns: &[String]) -> Result<(), ConfigurationError> {
    for (index, pattern) in patterns.iter().enumerate() {
        if !valid_mutator_pattern(pattern) {
            return Err(ConfigurationError::new(format!(
                "{field}[{index}] must be a mutator name or a group pattern ending in /*"
            )));
        }
    }
    Ok(())
}

fn valid_mutator_pattern(pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let Some(prefix) = pattern.strip_suffix('*') else {
        return valid_mutator_name(pattern);
    };
    prefix.ends_with('/')
        && !prefix.is_empty()
        && !prefix.contains('*')
        && valid_mutator_name(&prefix[..prefix.len() - 1])
}

fn valid_mutator_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('/').all(|part| {
            !part.is_empty()
                && part.chars().all(|character| {
                    character.is_ascii_lowercase()
                        || character.is_ascii_digit()
                        || character == '-'
                        || character == '_'
                })
        })
}

fn validate_regular_expressions(patterns: &[String]) -> Result<(), ConfigurationError> {
    for (index, pattern) in patterns.iter().enumerate() {
        if let Err(error) = Regex::new(pattern) {
            return Err(ConfigurationError::new(format!(
                "ignore_source_lines[{index}] has an invalid regular expression: {error}"
            )));
        }
    }
    Ok(())
}

fn validate_matching_patterns(
    field: &str,
    patterns: &[String],
    known_names: &[String],
) -> Result<(), ConfigurationError> {
    for pattern in patterns {
        if !known_names
            .iter()
            .any(|name| mutator_pattern_matches(pattern, name))
        {
            return Err(ConfigurationError::new(format!(
                "{field} pattern {pattern:?} does not match an available mutator"
            )));
        }
    }
    Ok(())
}

fn mutator_pattern_matches(pattern: &str, name: &str) -> bool {
    pattern
        .strip_suffix('*')
        .map_or_else(|| pattern == name, |prefix| name.starts_with(prefix))
}
