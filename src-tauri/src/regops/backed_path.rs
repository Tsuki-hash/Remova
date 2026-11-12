use std::collections::BTreeMap;

const SCOPES: [&str; 2] = ["User", "Machine"];

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct PathValue {
    raw: String,
    expand: bool,
    // Preserve the object shape so older versions can still read the snapshot.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    missing: bool,
}

impl PathValue {
    fn missing() -> Self {
        Self {
            raw: String::new(),
            expand: true,
            missing: true,
        }
    }

    fn read(scope: &str) -> Result<Self, String> {
        #[cfg(test)]
        if super::path_mock::active() {
            return Ok(super::path_mock::read_present_raw(scope)?.map_or_else(
                Self::missing,
                |(raw, expand)| Self {
                    raw,
                    expand,
                    missing: false,
                },
            ));
        }
        #[cfg(windows)]
        {
            use windows::Win32::System::Registry::{REG_EXPAND_SZ, REG_SZ};
            let Some((bytes, kind)) =
                super::read_reg_value_bytes(super::path_scope_key(scope)?, "Path")?
            else {
                return Ok(Self::missing());
            };
            if kind != REG_SZ.0 && kind != REG_EXPAND_SZ.0 {
                return Err(format!("unexpected type for Path {scope}"));
            }
            Ok(Self {
                raw: super::decode_reg_utf16(&bytes)?,
                expand: kind == REG_EXPAND_SZ.0,
                missing: false,
            })
        }
        #[cfg(not(windows))]
        Err(format!("cannot read Path {scope} outside Windows"))
    }

    fn expanded(&self) -> String {
        if self.expand {
            super::expand_env_string(&self.raw)
        } else {
            self.raw.clone()
        }
    }

    fn matches_removals(&self, removed: &BTreeMap<usize, &str>) -> bool {
        let segments: Vec<_> = self.raw.split(';').collect();
        removed.iter().all(|(&index, entry)| {
            super::normalize_path_entry(entry).is_some_and(|needle| {
                segments.get(index).is_some_and(|segment| {
                    super::path_segment_matches(segment, self.expand, &needle)
                })
            })
        })
    }

    fn without_removals(&self, removed: &BTreeMap<usize, &str>) -> Self {
        let raw = self
            .raw
            .split(';')
            .enumerate()
            .filter_map(|(i, segment)| (!removed.contains_key(&i)).then_some(segment))
            .collect::<Vec<_>>()
            .join(";");
        Self {
            raw,
            expand: self.expand,
            missing: self.missing,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct BackedPathPlan {
    values: [PathValue; 2],
    // Each position belongs to the exact raw and typed PATH captured above.
    entries: BTreeMap<String, [Vec<usize>; 2]>,
}

pub(crate) enum BackedPathRestore {
    Restored(&'static str),
    AlreadyOriginal(&'static str),
    Preserved(&'static str),
    Failed(&'static str, String),
    Cancelled,
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

    /// Both cleanup and recovery project the same unambiguous change from
    /// the authenticated positions. An originally repeated match is kept.
    fn removals(&self, scope_idx: usize) -> BTreeMap<usize, &str> {
        let mut removed = BTreeMap::new();
        for (entry, positions) in &self.entries {
            let [index] = positions[scope_idx].as_slice() else {
                continue;
            };
            if !crate::policy::is_dangerous_path_entry(entry) {
                removed.entry(*index).or_insert(entry.as_str());
            }
        }
        removed
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
            let remove = self.removals(scope_idx);
            // If even one selected segment no longer resolves to the approved
            // path, leave this scope intact rather than producing a different
            // partial value than the one recovery can verify.
            let can_remove = !current.missing
                && current == self.values[scope_idx]
                && current.matches_removals(&remove);
            for (entry, positions) in &self.entries {
                let Some(needle) = super::normalize_path_entry(entry) else {
                    continue;
                };
                let kept = match positions[scope_idx].as_slice() {
                    [index] if can_remove && remove.contains_key(index) => false,
                    [] => current.raw.split(';').any(|segment| {
                        super::path_segment_matches(segment, current.expand, &needle)
                    }),
                    _ => true,
                };
                results.get_mut(entry).unwrap().kept |= kept;
            }
            if !can_remove || remove.is_empty() {
                continue;
            }
            let value = current.without_removals(&remove);
            // A smaller result could fit even when the original would be
            // refused on restore. Only clean a scope that can be written back.
            let write = super::validate_path_scope_value(&current.raw)
                .and_then(|()| super::write_path_scope(scope, &value.raw, value.expand));
            for entry in remove.into_values() {
                let result = results.get_mut(entry).unwrap();
                match &write {
                    Ok(()) => result.removed = true,
                    Err(error) => {
                        result.kept = true;
                        result.errors.push(format!("{scope}: {error}"));
                    }
                }
            }
        }
        if results.values().any(|result| result.removed) {
            super::broadcast_env_change();
        }
        results
    }

    pub(crate) fn restore(&self) -> Vec<BackedPathRestore> {
        use BackedPathRestore::*;
        let _lock = super::PATH_LOCK
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let mut results = Vec::new();
        for (scope_idx, scope) in SCOPES.into_iter().enumerate() {
            if crate::executor::cleanup_cancelled() {
                results.push(Cancelled);
                break;
            }
            let remove = self.removals(scope_idx);
            let original = &self.values[scope_idx];
            if original.missing || remove.is_empty() {
                continue;
            }
            let result = match PathValue::read(scope) {
                Ok(current) if current == *original => AlreadyOriginal(scope),
                Ok(current)
                    if current == original.without_removals(&remove)
                        && original.matches_removals(&remove) =>
                {
                    match super::write_path_scope(scope, &original.raw, original.expand) {
                        Ok(()) => Restored(scope),
                        Err(error) => Failed(scope, error),
                    }
                }
                Ok(_) => Preserved(scope),
                Err(error) => Failed(scope, error),
            };
            results.push(result);
        }
        if results.iter().any(|result| matches!(result, Restored(_))) {
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
        let plan = BackedPathPlan::capture(&[entry]).unwrap();
        let result = plan.scrub();
        assert!(result[entry].kept && !result[entry].removed);
        assert!(plan.restore().is_empty());
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
        let plan = BackedPathPlan::capture(&[first, second]).unwrap();
        let result = plan.scrub();
        assert!(result[first].removed && result[second].removed);
        assert_eq!(path_mock::get("User"), tail);
        assert_eq!(path_mock::get("Machine"), tail);
        assert!(result[first].errors.is_empty());
        let reopened: BackedPathPlan =
            serde_json::from_value(serde_json::to_value(plan).unwrap()).unwrap();
        assert!(matches!(
            reopened.restore().as_slice(),
            [
                BackedPathRestore::Restored("User"),
                BackedPathRestore::Restored("Machine")
            ]
        ));
        assert_eq!(path_mock::get("User"), format!("{first};{second};{tail}"));
        assert_eq!(path_mock::get("Machine"), format!("{first};{tail}"));
    }

    #[test]
    fn backed_path_restores_one_scope_and_reports_other_scope_changes() {
        let _lock = path_mock::lock_mock();
        crate::executor::begin_cleanup_control();
        let entry = r"C:\FixtureTools\One";
        let tail = r"C:\OtherTool\bin";
        let original = format!("{entry};{tail}");
        path_mock::install_expand(&original, &original);
        let plan = BackedPathPlan::capture(&[entry]).unwrap();
        assert!(plan.scrub()[entry].removed);

        let changed = format!(r"{tail};C:\External\bin");
        path_mock::install_expand(tail, &changed);
        assert!(matches!(
            plan.restore().as_slice(),
            [
                BackedPathRestore::Restored("User"),
                BackedPathRestore::Preserved("Machine")
            ]
        ));
        assert_eq!(path_mock::read_raw("User").unwrap(), (original, true));
        assert_eq!(path_mock::get("Machine"), changed);
        assert!(matches!(
            plan.restore().as_slice(),
            [
                BackedPathRestore::AlreadyOriginal("User"),
                BackedPathRestore::Preserved("Machine")
            ]
        ));
    }

    #[cfg(windows)]
    #[test]
    fn backed_path_plan_preserves_the_old_wire_shape_for_a_missing_scope() {
        #[derive(serde::Deserialize)]
        struct LegacyPathValue {
            raw: String,
            expand: bool,
        }

        #[derive(serde::Deserialize)]
        struct LegacyPlan {
            values: [LegacyPathValue; 2],
        }

        let _lock = path_mock::lock_mock();
        let entry = r"C:\FixtureTools\One";
        path_mock::install(entry, "");
        path_mock::remove_value("Machine");
        let plan = BackedPathPlan::capture(&[entry]).unwrap();
        let wire = serde_json::to_value(&plan).unwrap();
        let legacy: LegacyPlan = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(legacy.values[0].raw, entry);
        assert!(legacy.values[1].raw.is_empty() && legacy.values[1].expand);
        let reopened: BackedPathPlan = serde_json::from_value(wire).unwrap();
        assert_eq!(reopened.values, plan.values);
    }

    #[cfg(windows)]
    #[test]
    fn backed_path_restore_distinguishes_empty_missing_type_and_expansion() {
        let _lock = path_mock::lock_mock();
        crate::executor::begin_cleanup_control();
        let root = r"C:\FixtureTools\One";
        let entry = r"C:\FixtureTools\One\bin";
        let raw = r"%FIXTURE_ROOT%\bin";
        super::super::expand_mock::install(&[("FIXTURE_ROOT", root)]);
        path_mock::install_expand(raw, "");
        let plan = BackedPathPlan::capture(&[entry]).unwrap();
        assert!(plan.scrub()[entry].removed);
        assert_eq!(path_mock::read_raw("User").unwrap(), (String::new(), true));

        path_mock::remove_value("User");
        assert!(matches!(
            plan.restore().as_slice(),
            [BackedPathRestore::Preserved("User")]
        ));
        assert!(path_mock::read_present_raw("User").unwrap().is_none());
        path_mock::install("", "");
        assert!(matches!(
            plan.restore().as_slice(),
            [BackedPathRestore::Preserved("User")]
        ));
        assert_eq!(path_mock::read_raw("User").unwrap(), (String::new(), false));

        path_mock::install_expand("", "");
        super::super::expand_mock::install(&[("FIXTURE_ROOT", r"C:\External")]);
        assert!(matches!(
            plan.restore().as_slice(),
            [BackedPathRestore::Preserved("User")]
        ));
        super::super::expand_mock::install(&[("FIXTURE_ROOT", root)]);
        assert!(matches!(
            plan.restore().as_slice(),
            [BackedPathRestore::Restored("User")]
        ));
        assert_eq!(path_mock::read_raw("User").unwrap(), (raw.into(), true));
    }

    #[test]
    fn backed_path_leaves_a_scope_intact_when_its_original_value_cannot_be_written_back() {
        let _lock = path_mock::lock_mock();
        crate::executor::begin_cleanup_control();
        let entry = r"C:\FixtureTools\One";
        let tail = "x".repeat(30_000 - entry.len());
        let original = format!("{entry};{tail}");
        path_mock::install(&original, entry);
        let plan = BackedPathPlan::capture(&[entry]).unwrap();

        let results = plan.scrub();
        assert!(results[entry].kept && results[entry].removed);
        assert!(results[entry].errors[0].contains("PATH value exceeds"));
        assert_eq!(path_mock::get("User"), original);
        assert_eq!(path_mock::get("Machine"), "");
        assert!(matches!(
            plan.restore().as_slice(),
            [
                BackedPathRestore::AlreadyOriginal("User"),
                BackedPathRestore::Restored("Machine")
            ]
        ));
        assert_eq!(path_mock::get("Machine"), entry);
    }

    #[test]
    fn backed_path_restore_preserves_an_original_scope_after_a_failed_cleanup_write() {
        let _lock = path_mock::lock_mock();
        crate::executor::begin_cleanup_control();
        let entry = r"C:\FixtureTools\One";
        path_mock::install(entry, entry);
        let plan = BackedPathPlan::capture(&[entry]).unwrap();
        path_mock::fail_machine_write();
        let removed = plan.scrub();
        assert!(removed[entry].removed && !removed[entry].errors.is_empty());
        assert!(matches!(
            plan.restore().as_slice(),
            [
                BackedPathRestore::Restored("User"),
                BackedPathRestore::AlreadyOriginal("Machine")
            ]
        ));
        assert_eq!(path_mock::get("User"), entry);
        assert_eq!(path_mock::get("Machine"), entry);
    }

    #[test]
    fn backed_path_restore_reports_a_failed_write_without_losing_the_other_scope() {
        let _lock = path_mock::lock_mock();
        crate::executor::begin_cleanup_control();
        let entry = r"C:\FixtureTools\One";
        path_mock::install(entry, entry);
        let plan = BackedPathPlan::capture(&[entry]).unwrap();
        assert!(plan.scrub()[entry].removed);
        path_mock::fail_machine_write();
        assert!(
            matches!(plan.restore().as_slice(), [BackedPathRestore::Restored("User"), BackedPathRestore::Failed("Machine", error)] if error.contains("injected Machine write failure"))
        );
        assert_eq!(path_mock::get("User"), entry);
        assert_eq!(path_mock::get("Machine"), "");
    }
}
