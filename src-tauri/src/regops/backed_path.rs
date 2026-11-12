use std::collections::{btree_map::Entry, BTreeMap};

const SCOPES: [&str; 2] = ["User", "Machine"];

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct PathValue {
    raw: String,
    expand: bool,
}

impl PathValue {
    fn read(scope: &str) -> Result<Self, String> {
        let (raw, expand) = super::read_path_scope_raw(scope)?;
        Ok(Self { raw, expand })
    }

    fn expanded(&self) -> String {
        if self.expand {
            super::expand_env_string(&self.raw)
        } else {
            self.raw.clone()
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct BackedPathPlan {
    values: [PathValue; 2],
    // Each position belongs to the exact raw and typed PATH captured above.
    entries: BTreeMap<String, [Vec<usize>; 2]>,
}

#[derive(Default)]
pub(crate) struct BackedPathResult {
    pub(crate) removed: bool,
    pub(crate) kept: bool,
    pub(crate) errors: Vec<String>,
}

impl BackedPathPlan {
    pub(crate) fn capture(entries: &[&str]) -> Result<Self, String> {
        let _lock = super::PATH_LOCK
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let values = [PathValue::read(SCOPES[0])?, PathValue::read(SCOPES[1])?];
        let entries = entries
            .iter()
            .map(|entry| {
                let needle = super::normalize_path_entry(entry).ok_or("invalid PATH entry")?;
                let positions = values.each_ref().map(|value| {
                    value
                        .raw
                        .split(';')
                        .enumerate()
                        .filter_map(|(i, segment)| {
                            super::path_segment_matches(segment, value.expand, &needle).then_some(i)
                        })
                        .collect()
                });
                Ok((entry.to_string(), positions))
            })
            .collect::<Result<_, String>>()?;
        Ok(Self { values, entries })
    }

    pub(crate) fn expanded_scopes(&self) -> [String; 2] {
        self.values.each_ref().map(PathValue::expanded)
    }

    pub(crate) fn entry_scopes(&self, entry: &str) -> Vec<String> {
        self.entries
            .get(entry)
            .into_iter()
            .flat_map(|positions| {
                SCOPES
                    .into_iter()
                    .zip(positions)
                    .filter(|(_, positions)| !positions.is_empty())
                    .map(|(scope, _)| scope.to_string())
            })
            .collect()
    }

    pub(crate) fn scrub(&self) -> BTreeMap<String, BackedPathResult> {
        let _lock = super::PATH_LOCK
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let mut results: BTreeMap<_, _> = self
            .entries
            .keys()
            .map(|entry| (entry.clone(), BackedPathResult::default()))
            .collect();
        for (scope_idx, scope) in SCOPES.into_iter().enumerate() {
            if crate::executor::cleanup_cancelled() {
                break;
            }
            let current = match PathValue::read(scope) {
                Ok(current) => current,
                Err(error) => {
                    for (entry, positions) in &self.entries {
                        if !positions[scope_idx].is_empty() {
                            results
                                .get_mut(entry)
                                .unwrap()
                                .errors
                                .push(format!("{scope}: {error}"));
                        }
                    }
                    continue;
                }
            };
            let segments: Vec<_> = current.raw.split(';').collect();
            let unchanged = current == self.values[scope_idx];
            let mut remove = BTreeMap::new();
            for (entry, positions) in &self.entries {
                let Some(needle) = super::normalize_path_entry(entry) else {
                    continue;
                };
                let matches =
                    |segment| super::path_segment_matches(segment, current.expand, &needle);
                // The normal restore merges a missing entry once. A duplicate
                // in the original scope cannot be restored at its multiplicity.
                match positions[scope_idx].as_slice() {
                    [index]
                        if unchanged
                            && segments.get(*index).is_some_and(|segment| matches(segment)) =>
                    {
                        if let Entry::Vacant(slot) = remove.entry(*index) {
                            slot.insert(entry);
                        }
                    }
                    [] if !segments.iter().any(|segment| matches(segment)) => {}
                    _ => results.get_mut(entry).unwrap().kept = true,
                }
            }
            if remove.is_empty() {
                continue;
            }
            let value = segments
                .into_iter()
                .enumerate()
                .filter_map(|(i, segment)| (!remove.contains_key(&i)).then_some(segment))
                .collect::<Vec<_>>()
                .join(";");
            let write = super::write_path_scope(scope, &value, current.expand);
            for entry in remove.into_values() {
                let result = results.get_mut(entry).unwrap();
                match &write {
                    Ok(()) => result.removed = true,
                    Err(error) => result.errors.push(format!("{scope}: {error}")),
                }
            }
        }
        if results.values().any(|result| result.removed) {
            super::broadcast_env_change();
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::regops::path_mock;

    #[test]
    fn backed_path_skips_changed_order_duplicates_and_registry_value_type() {
        let _lock = path_mock::lock_mock();
        crate::executor::begin_cleanup_control();
        let entry = r"C:\FixtureTools\One";
        let other = r"C:\FixtureTools\Two";
        let initial = format!("{entry};{other}");
        for changed in [
            format!("{other};{entry}"),
            format!("{entry};{entry};{other}"),
            initial.clone(),
        ] {
            path_mock::install(&initial, "");
            let snapshot = BackedPathPlan::capture(&[entry]).unwrap();
            if changed == initial {
                path_mock::install_expand(&changed, "");
            } else {
                path_mock::install(&changed, "");
            }
            let result = snapshot.scrub();
            assert!(result[entry].kept && !result[entry].removed);
            assert_eq!(path_mock::get("User"), changed);
        }
        let duplicates = format!("{entry};{entry}");
        path_mock::install(&duplicates, "");
        let result = BackedPathPlan::capture(&[entry]).unwrap().scrub();
        assert!(result[entry].kept && !result[entry].removed);
        assert_eq!(path_mock::get("User"), duplicates);
    }

    #[test]
    fn backed_path_scrubs_multiple_entries_against_one_unchanged_scope_version() {
        let _lock = path_mock::lock_mock();
        crate::executor::begin_cleanup_control();
        let first = r"C:\FixtureTools\One";
        let second = r"C:\FixtureTools\Two";
        let tail = r"C:\FixtureTools\Unrelated";
        path_mock::install(
            &format!("{first};{second};{tail}"),
            &format!("{first};{tail}"),
        );
        let result = BackedPathPlan::capture(&[first, second]).unwrap().scrub();
        assert!(result[first].removed && result[second].removed);
        assert_eq!(path_mock::get("User"), tail);
        assert_eq!(path_mock::get("Machine"), tail);
    }
}
