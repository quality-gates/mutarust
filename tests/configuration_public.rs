use std::fs;

use mutarust::{CommandSettings, Configuration, Registry};

struct TempConfig(std::path::PathBuf);

impl Drop for TempConfig {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[test]
fn exact_assign_invert_selectors_work_in_command_settings_and_yaml() {
    let names = Registry::builtins()
        .names()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut command_configuration = Configuration::default();
    command_configuration
        .apply(&CommandSettings {
            enable_mutators: Some(vec!["arithmetic/assign_invert".to_owned()]),
            ..CommandSettings::default()
        })
        .expect("the exact command selector must be valid");
    assert_eq!(
        command_configuration
            .select_mutators(&names)
            .expect("the exact command selector must match"),
        vec!["arithmetic/assign_invert"]
    );

    let path = TempConfig(std::env::temp_dir().join(format!(
        "mutarust-underscore-selector-{}.yml",
        std::process::id()
    )));
    fs::write(&path.0, "disable_mutators:\n  - arithmetic/assign_invert\n")
        .expect("the selector fixture must be written");
    let yaml_configuration =
        Configuration::read(&path.0).expect("the exact YAML selector must be valid");
    assert!(
        !yaml_configuration
            .select_mutators(&names)
            .expect("the exact YAML selector must match")
            .contains(&"arithmetic/assign_invert".to_owned())
    );
}
