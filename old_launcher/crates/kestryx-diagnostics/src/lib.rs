//! `kestryx-diagnostics`: Community-extensible Crash Assistant (`data/crash-rules.json`),
//! `hs_err_pid*.log` / `latest.log` / crash-report analyzer, one-click remediation builder,
//! Instance Doctor, and redacted diagnostics exporter.

use std::collections::BTreeMap;

use anyhow::Result;
use kestryx_auth::SecretRedactor;
use kestryx_java::class_file_version_to_java_major;
use regex::Regex;
use serde::{Deserialize, Serialize};

const EMBEDDED_CRASH_RULES_JSON: &str = include_str!("../../../data/crash-rules.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashRuleSet {
    pub schema_version: u32,
    pub rules: Vec<CrashRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashRule {
    pub id: String,
    pub title: String,
    pub severity: String,
    pub category: String,
    pub patterns: Vec<String>,
    pub cause_explanation: String,
    pub fix: CrashFixSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashFixSpec {
    pub action_kind: String,
    pub label: String,
    #[serde(default)]
    pub recommended_delta_mb: Option<u32>,
    #[serde(default)]
    pub target_major: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashDiagnosis {
    pub rule_id: String,
    pub title: String,
    pub severity: String,
    pub category: String,
    pub cause_explanation: String,
    pub matched_line: String,
    pub one_click_fix: OneClickFixAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OneClickFixAction {
    IncreaseRam { new_max_mb: u32, label: String },
    SwitchJavaRuntime { required_major: u32, label: String },
    InstallMissingModDependency { mod_id: String, label: String },
    DisableDuplicateMod { mod_id: String, label: String },
    DisableConflictingMod { target: String, label: String },
    EnableSafeGpuFlags { dll_name: Option<String>, label: String },
}

pub struct CrashAnalyzer {
    ruleset: CrashRuleSet,
}

impl CrashAnalyzer {
    pub fn load_default() -> Result<Self> {
        let ruleset: CrashRuleSet = serde_json::from_str(EMBEDDED_CRASH_RULES_JSON)?;
        Ok(Self { ruleset })
    }

    pub fn from_json_str(raw: &str) -> Result<Self> {
        let ruleset: CrashRuleSet = serde_json::from_str(raw)?;
        Ok(Self { ruleset })
    }

    /// Analyzes a crash report, `hs_err` log, or `latest.log` and returns all matched diagnoses
    /// with concrete one-click fixes.
    pub fn analyze(&self, log_text: &str, current_max_ram_mb: u32) -> Vec<CrashDiagnosis> {
        let mut diagnoses = Vec::new();

        for rule in &self.ruleset.rules {
            for pat in &rule.patterns {
                let Ok(re) = Regex::new(pat) else {
                    continue;
                };
                if let Some(caps) = re.captures(log_text) {
                    let matched_line = caps
                        .get(0)
                        .map(|m| m.as_str().to_string())
                        .unwrap_or_default();

                    let one_click_fix = match rule.fix.action_kind.as_str() {
                        "increase_ram" => {
                            let delta = rule.fix.recommended_delta_mb.unwrap_or(2048);
                            let new_max = (current_max_ram_mb + delta).min(16384);
                            OneClickFixAction::IncreaseRam {
                                new_max_mb: new_max,
                                label: format!("Increase RAM to {new_max} MB"),
                            }
                        }
                        "switch_java_for_class_version" => {
                            let class_ver = caps
                                .name("class_ver")
                                .and_then(|m| m.as_str().parse::<u32>().ok())
                                .unwrap_or(65);
                            let req_major = class_file_version_to_java_major(class_ver);
                            OneClickFixAction::SwitchJavaRuntime {
                                required_major: req_major,
                                label: format!("Switch to Java {req_major} Runtime"),
                            }
                        }
                        "switch_java_major" => {
                            let req_major = rule.fix.target_major.unwrap_or(8);
                            OneClickFixAction::SwitchJavaRuntime {
                                required_major: req_major,
                                label: rule.fix.label.clone(),
                            }
                        }
                        "install_missing_dependency" => {
                            let dep_id = caps
                                .name("dep_id")
                                .map(|m| m.as_str().to_string())
                                .unwrap_or_else(|| "fabric-api".to_string());
                            OneClickFixAction::InstallMissingModDependency {
                                label: format!("Install '{dep_id}' from Modrinth"),
                                mod_id: dep_id,
                            }
                        }
                        "disable_older_duplicate_mod" => {
                            let mod_id = caps
                                .name("mod_id")
                                .map(|m| m.as_str().to_string())
                                .unwrap_or_else(|| "duplicate".to_string());
                            OneClickFixAction::DisableDuplicateMod {
                                label: format!("Disable Older Duplicate of '{mod_id}'"),
                                mod_id,
                            }
                        }
                        "isolate_culprit_mod" => {
                            let target = caps
                                .name("mod_id")
                                .or_else(|| caps.name("mixin_json"))
                                .map(|m| m.as_str().to_string())
                                .unwrap_or_else(|| "conflicting-mod".to_string());
                            OneClickFixAction::DisableConflictingMod {
                                label: format!("Disable '{target}' & Retry"),
                                target,
                            }
                        }
                        _ => {
                            let dll = caps.name("dll").map(|m| m.as_str().to_string());
                            OneClickFixAction::EnableSafeGpuFlags {
                                dll_name: dll,
                                label: rule.fix.label.clone(),
                            }
                        }
                    };

                    diagnoses.push(CrashDiagnosis {
                        rule_id: rule.id.clone(),
                        title: rule.title.clone(),
                        severity: rule.severity.clone(),
                        category: rule.category.clone(),
                        cause_explanation: rule.cause_explanation.clone(),
                        matched_line,
                        one_click_fix,
                    });
                    break;
                }
            }
        }

        diagnoses
    }
}

/// Builds a redacted diagnostics bundle safe for pasting to GitHub or Discord.
pub fn build_redacted_diagnostics_bundle(
    instance_id: &str,
    mc_version: &str,
    loader: &str,
    java_info: &str,
    log_tail: &str,
    diagnoses: &[CrashDiagnosis],
    secrets_to_redact: Vec<String>,
) -> String {
    let redactor = SecretRedactor::new(secrets_to_redact);
    let mut extra = BTreeMap::new();
    extra.insert("os", std::env::consts::OS);
    extra.insert("arch", std::env::consts::ARCH);

    let raw_bundle = format!(
        "=== Kestryx Diagnostics Bundle ===\n\
         OS/Arch: {}/{}\n\
         Instance: {} (MC {}, Loader: {})\n\
         Java: {}\n\
         Diagnoses ({}):\n{}\n\
         --- Log Tail ---\n{}\n",
        extra["os"],
        extra["arch"],
        instance_id,
        mc_version,
        loader,
        java_info,
        diagnoses.len(),
        serde_json::to_string_pretty(diagnoses).unwrap_or_default(),
        log_tail
    );

    redactor.redact(&raw_bundle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crash_analyzer_detects_oom_java_version_and_missing_deps() -> Result<()> {
        let analyzer = CrashAnalyzer::load_default()?;

        let sample_log = r#"
[14:20:01] [main/ERROR]: Mixin apply for mod sodium_extra failed
java.lang.UnsupportedClassVersionError: net/minecraft/client/main/Main has been compiled by a more recent version of the Java Runtime (class file version 69.0), this version of the Java Runtime only recognizes class file versions up to 65.0
java.lang.OutOfMemoryError: Java heap space
Mod 'Iris' (iris) 1.8.0 requires any version of 'sodium', which is missing!
"#;

        let diags = analyzer.analyze(sample_log, 4096);
        assert!(diags.iter().any(|d| d.rule_id == "oom-heap-space"
            && d.one_click_fix
                == OneClickFixAction::IncreaseRam {
                    new_max_mb: 6144,
                    label: "Increase RAM to 6144 MB".to_string()
                }));
        assert!(diags.iter().any(|d| d.rule_id == "wrong-java-class-version"
            && d.one_click_fix
                == OneClickFixAction::SwitchJavaRuntime {
                    required_major: 25,
                    label: "Switch to Java 25 Runtime".to_string()
                }));
        assert!(diags.iter().any(|d| d.rule_id == "missing-mod-dependency"));
        assert!(diags.iter().any(|d| d.rule_id == "mixin-injection-failure"));
        Ok(())
    }
}
