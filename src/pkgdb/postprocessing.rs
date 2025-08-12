use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::Read,
    path::Path,
};
use serde::Deserialize;

use crate::pkgdb::Package;

pub fn extend_string_with_separator(original: &mut String, extend_with: &str, separator: char) {
    if original.len() == 0 {
        original.push_str(extend_with);
    } else {
        original.push_str(&format!("{}{}", separator, extend_with));
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct Postprocessing {
    new_package: Option<Vec<Package>>,
    // Merge packages into a new one with name = key and packages to be merged as value
    merge_packages: Option<HashMap<String, Vec<String>>>,
}

impl Postprocessing {
    pub(crate) fn new_from_toml<P: AsRef<Path>>(path: P) -> Result<Self, anyhow::Error> {
        let mut file = File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        Ok(toml::from_str(&contents)?)
    }

    pub(crate) fn apply(self, mut index: Vec<Package>) -> Result<Vec<Package>, anyhow::Error> {
        if let Some(new_packages) = self.new_package {
            index.extend(new_packages);
        }
        if let Some(merge_packages) = self.merge_packages {
            for (new_name, packages_to_merge) in merge_packages {
                // Find the affected packages and remember their indices.
                // Create a merged package inside the `fold` part.
                let (indices, mut merged_package) = index
                    .iter_mut()
                    .enumerate()
                    .filter(|(_i, pkg)| packages_to_merge.contains(&pkg.name))
                    .fold(
                        (Vec::new(), Package::default()),
                        |(mut indices, mut merged_package), (i, package)| {
                            // Remember index to remove it later
                            indices.push(i);
                            // Merge identifier, source and version fields
                            extend_string_with_separator(
                                &mut merged_package.identifier,
                                &package.identifier,
                                ',',
                            );
                            extend_string_with_separator(
                                &mut merged_package.source,
                                &package.source,
                                ',',
                            );
                            extend_string_with_separator(
                                &mut merged_package.version,
                                &package.version,
                                ',',
                            );
                            // Sum sizes
                            merged_package.size += package.size;
                            // Take all files from the merged package.
                            // This will empty the files list, which is fine, because we will remove it later, anyway
                            merged_package.files.extend(package.files.drain(..));
                            (indices, merged_package)
                        },
                    );
                // Set the name of the merged package as indicated by the user
                merged_package.name = new_name;
                // Files are deduplicated before merging
                merged_package.files = merged_package
                    .files
                    .into_iter()
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect();
                // Delete the packages that were just merged from the index
                for i in 0..indices.len() {
                    let _ = index.remove(indices[i] - i);
                }
                index.push(merged_package);
            }
        }
        Ok(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_deserialize_postprocessing() {
        let toml_str = r#"
[[new_package]]
identifier = "initramfs"
name = "initramfs"
version = "1"
source = "initramfs"
size = 209715200
files = [
    "/usr/lib/modules/6.15.9-arch1-1/initramfs.img"
]

[merge_packages]
"basepkg" = ["base", "filesystem"]
"certificates" = ["ca-certificates", "ca-certificates-mozilla", "ca-certificates-utils"]
"dbuspkg" = ["dbus", "dbus-broker", "dbus-units"]
"#;

        let post: Postprocessing = toml::from_str(toml_str).expect("Failed to deserialize TOML");

        // Validate new_package
        let pkgs = post
            .new_package
            .expect("Expected new_packages to be present");
        assert_eq!(pkgs.len(), 1);

        assert_eq!(pkgs[0].identifier, "initramfs");
        assert_eq!(pkgs[0].name, "initramfs");
        assert_eq!(pkgs[0].version, "1");
        assert_eq!(pkgs[0].source, "initramfs");
        assert_eq!(pkgs[0].size, 209715200);
        assert_eq!(pkgs[0].files.len(), 1);
        assert_eq!(
            pkgs[0].files[0].as_str(),
            "/usr/lib/modules/6.15.9-arch1-1/initramfs.img"
        );

        // Validate merge_packages
        let merges = post
            .merge_packages
            .expect("Expected merge_packages to be present");
        assert_eq!(merges.len(), 3);

        assert_eq!(
            merges.get("basepkg").unwrap(),
            &vec!["base".to_string(), "filesystem".to_string()]
        );
        assert_eq!(
            merges.get("certificates").unwrap(),
            &vec![
                "ca-certificates".to_string(),
                "ca-certificates-mozilla".to_string(),
                "ca-certificates-utils".to_string()
            ]
        );
    }
}
