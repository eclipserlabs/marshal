#![allow(missing_docs)]
//! Policy as Code — `marshall.yaml` (Phase 3)
//!
//! ```yaml
//! workspace: /tmp/marshalld
//! concurrency: 32
//! audit_log: ./audit.jsonl
//! filesystem:
//!   writable: true
//! shell:
//!   commands:
//!     - program: /bin/echo
//!       args: NoFlags
//!     - program: /bin/cat
//!       args: { Exact: [["--help"]] }
//!     - program: /usr/bin/git
//!       args: { Exact: [["status"]] }
//! http:
//!   allowed_hosts: [api.github.com]
//!   request_body_limit: 1048576
//!   response_body_limit: 4194304
//! ```

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{shell::AllowedCommand, ArgumentPolicy, Sandbox};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPolicy {
    #[serde(default = "default_workspace")]
    pub workspace: PathBuf,
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
    pub audit_log: Option<PathBuf>,
    #[serde(default)]
    pub filesystem: FilesystemPolicy,
    #[serde(default)]
    pub shell: ShellPolicy,
    #[serde(default)]
    pub http: HttpPolicy,
    #[serde(default)]
    pub code: CodePolicy,
    #[serde(default)]
    pub system: SystemPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemPolicy {
    #[serde(default = "default_true")]
    pub writable: bool,
    #[serde(default = "default_read_limit")]
    pub read_limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellPolicy {
    #[serde(default)]
    pub commands: Vec<ShellCommandPolicy>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default = "default_output_limit")]
    pub output_limit: usize,
    pub allowed_env: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellCommandPolicy {
    pub program: String,
    #[serde(default = "default_arg_policy")]
    pub args: ArgPolicySerde,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgPolicySerde {
    Simple(String),
    Detailed(ArgPolicyDetailed),
}

#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArgPolicyDetailed {
    pub Exact: Option<Vec<Vec<String>>>,
    pub NoFlags: Option<bool>,
    pub Unrestricted: Option<bool>,
    pub None: Option<bool>,
}

impl ArgPolicySerde {
    pub fn into_policy(self) -> ArgumentPolicy {
        match self {
            ArgPolicySerde::Simple(s) => match s.as_str() {
                "None" => ArgumentPolicy::None,
                "NoFlags" => ArgumentPolicy::NoFlags,
                "Unrestricted" => ArgumentPolicy::Unrestricted,
                _ => ArgumentPolicy::None,
            },
            ArgPolicySerde::Detailed(d) => {
                if let Some(v) = d.Exact {
                    return ArgumentPolicy::Exact(v);
                }
                if d.NoFlags == Some(true) {
                    return ArgumentPolicy::NoFlags;
                }
                if d.Unrestricted == Some(true) {
                    return ArgumentPolicy::Unrestricted;
                }
                ArgumentPolicy::None
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpPolicy {
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    #[serde(default = "default_req_limit")]
    pub request_body_limit: usize,
    #[serde(default = "default_resp_limit")]
    pub response_body_limit: usize,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

/// Policy for the `code` tool.
///
/// # Why this one has an extra gate
///
/// `shell` is an allowlist of *binaries*: the policy names what may run.
/// `code` accepts arbitrary source in an allowed language, so on the local
/// backend it is equivalent to `shell` with `ArgumentPolicy::Unrestricted` on
/// an interpreter — a snippet reads any file the daemon can read and reaches
/// any host the daemon can reach, bypassing `filesystem` and `http` policy
/// entirely.
///
/// So enabling a language is not enough. [`CodePolicy::allow_unsandboxed`]
/// must also be set, which is the operator saying in the config file that they
/// know the other policies do not apply here. Isolating backends (`wasm`,
/// `container`) do not need it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodePolicy {
    /// Languages the tool will run. Empty means the tool is not registered.
    #[serde(default)]
    pub allowed_languages: Vec<String>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default = "default_output_limit")]
    pub output_limit: usize,
    /// Acknowledge that local-backend code execution has no OS isolation.
    ///
    /// Required to enable [`CodePolicy::allowed_languages`] on the local
    /// backend; without it, a config that lists languages fails to load.
    #[serde(default)]
    pub allow_unsandboxed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemPolicy {
    #[serde(default)]
    pub allowed_env: Vec<String>,
    #[serde(default)]
    pub allow_process_list: bool,
    #[serde(default)]
    pub allow_kill: bool,
    #[serde(default = "default_max_sleep")]
    pub max_sleep_ms: u64,
}

// defaults
fn default_workspace() -> PathBuf {
    PathBuf::from("/tmp/marshalld")
}
fn default_concurrency() -> usize {
    32
}
fn default_true() -> bool {
    true
}
fn default_read_limit() -> usize {
    8 * 1024 * 1024
}
fn default_timeout() -> u64 {
    30_000
}
fn default_output_limit() -> usize {
    1024 * 1024
}
fn default_req_limit() -> usize {
    4 * 1024 * 1024
}
fn default_resp_limit() -> usize {
    4 * 1024 * 1024
}
fn default_arg_policy() -> ArgPolicySerde {
    ArgPolicySerde::Simple("None".into())
}
fn default_max_sleep() -> u64 {
    5_000
}

impl Default for ExecutionPolicy {
    fn default() -> Self {
        Self {
            workspace: default_workspace(),
            concurrency: default_concurrency(),
            audit_log: None,
            filesystem: FilesystemPolicy::default(),
            shell: ShellPolicy::default(),
            http: HttpPolicy::default(),
            code: CodePolicy::default(),
            system: SystemPolicy::default(),
        }
    }
}
impl Default for FilesystemPolicy {
    fn default() -> Self {
        Self {
            writable: true,
            read_limit: default_read_limit(),
        }
    }
}
impl Default for ShellPolicy {
    fn default() -> Self {
        Self {
            commands: vec![],
            timeout_ms: default_timeout(),
            output_limit: default_output_limit(),
            allowed_env: None,
        }
    }
}
impl Default for HttpPolicy {
    fn default() -> Self {
        Self {
            allowed_hosts: vec![],
            request_body_limit: default_req_limit(),
            response_body_limit: default_resp_limit(),
            timeout_ms: default_timeout(),
        }
    }
}
impl Default for CodePolicy {
    fn default() -> Self {
        Self {
            // Deny by default, like every other allowlist in this crate. The
            // previous default enabled python/bash/javascript, which made an
            // unconfigured daemon strictly more permissive than a configured
            // one.
            allowed_languages: Vec::new(),
            timeout_ms: default_timeout(),
            output_limit: default_output_limit(),
            allow_unsandboxed: false,
        }
    }
}
impl Default for SystemPolicy {
    fn default() -> Self {
        Self {
            allowed_env: vec![],
            allow_process_list: false,
            allow_kill: false,
            max_sleep_ms: default_max_sleep(),
        }
    }
}

impl ExecutionPolicy {
    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        let s = std::fs::read_to_string(path)?;
        Self::from_yaml(&s)
    }

    pub fn from_yaml(s: &str) -> anyhow::Result<Self> {
        let p: Self = serde_yaml::from_str(s)?;
        p.validate()?;
        Ok(p)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.concurrency == 0 {
            anyhow::bail!("concurrency must be >0");
        }
        if self.concurrency > 128 {
            anyhow::bail!("concurrency too large: {}", self.concurrency);
        }
        if self.workspace.as_os_str().is_empty() {
            anyhow::bail!("workspace must not be empty");
        }
        if !self.workspace.is_absolute() {
            anyhow::bail!("workspace must be absolute");
        }
        if self.filesystem.read_limit == 0 || self.filesystem.read_limit > 64 * 1024 * 1024 {
            anyhow::bail!("read_limit must be 1..64MiB");
        }
        if self.shell.timeout_ms == 0 || self.shell.timeout_ms > 300_000 {
            anyhow::bail!("shell timeout must be 1..300000ms");
        }
        if self.shell.output_limit == 0 || self.shell.output_limit > 16 * 1024 * 1024 {
            anyhow::bail!("shell output_limit must be 1..16MiB");
        }
        if self.http.timeout_ms == 0 || self.http.timeout_ms > 120_000 {
            anyhow::bail!("http timeout must be 1..120000ms");
        }
        for c in &self.shell.commands {
            if !c.program.starts_with('/') {
                anyhow::bail!("shell program must be absolute: {}", c.program);
            }
            if !Path::new(&c.program).is_absolute() {
                anyhow::bail!("absolute path required: {}", c.program);
            }
            // Reject weak policies for interpreters at load time (fail closed).
            crate::shell::validate_policy_for_program(&c.program, &c.args.clone().into_policy())?;
        }
        // hosts must be lowercased, no wildcards, no whitespace/control
        for h in &self.http.allowed_hosts {
            if h.chars().any(|c| c.is_control() || c.is_whitespace()) {
                anyhow::bail!("host contains control/whitespace: {h}");
            }
            if h.contains('*') {
                anyhow::bail!("wildcard hosts not allowed: {h}");
            }
            if h.len() > 253 {
                anyhow::bail!("host too long: {h}");
            }
        }
        if self.code.timeout_ms == 0 || self.code.timeout_ms > 30_000 {
            anyhow::bail!("code timeout must be 1..30000ms");
        }
        if self.code.output_limit == 0 || self.code.output_limit > 16 * 1024 * 1024 {
            anyhow::bail!("code output_limit must be 1..16MiB");
        }
        for lang in &self.code.allowed_languages {
            if !matches!(
                lang.as_str(),
                "python" | "javascript" | "js" | "bash" | "sh"
            ) {
                anyhow::bail!("unsupported code language: {lang}");
            }
        }
        // Fail closed: listing languages enables arbitrary source execution on
        // the local backend, which bypasses `filesystem` and `http` policy. The
        // operator has to say so explicitly.
        if !self.code.allowed_languages.is_empty() && !self.code.allow_unsandboxed {
            anyhow::bail!(
                "code.allowed_languages is set but code.allow_unsandboxed is false: \
                 local-backend code execution has no OS isolation and bypasses \
                 filesystem and http policy. Set code.allow_unsandboxed: true to \
                 accept this, or remove code.allowed_languages to disable the tool."
            );
        }
        if self.system.max_sleep_ms == 0
            || self.system.max_sleep_ms > crate::system::MAX_SLEEP_MS_HARD_CAP
        {
            anyhow::bail!("system max_sleep_ms must be 1..30000ms");
        }
        for key in &self.system.allowed_env {
            if key.is_empty() || key.len() > crate::system::MAX_ENV_KEY_LEN {
                anyhow::bail!("system allowed_env key too long: {key}");
            }
            if !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                anyhow::bail!("system allowed_env key invalid: {key}");
            }
            if key.bytes().next().is_some_and(|b| b.is_ascii_digit()) {
                anyhow::bail!("system allowed_env key invalid: {key}");
            }
        }
        Ok(())
    }

    pub fn sandbox(&self) -> anyhow::Result<Sandbox> {
        std::fs::create_dir_all(&self.workspace)?;
        Sandbox::new([&self.workspace]).map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub fn allowed_commands(&self) -> Vec<AllowedCommand> {
        self.shell
            .commands
            .iter()
            .map(|c| {
                AllowedCommand::new(c.program.clone()).with_arguments(c.args.clone().into_policy())
            })
            .collect()
    }

    pub fn shell_timeout(&self) -> Duration {
        Duration::from_millis(self.shell.timeout_ms)
    }
    pub fn http_timeout(&self) -> Duration {
        Duration::from_millis(self.http.timeout_ms)
    }

    pub fn code_timeout(&self) -> Duration {
        Duration::from_millis(self.code.timeout_ms)
    }

    pub fn code_languages(&self) -> Vec<crate::Language> {
        self.code
            .allowed_languages
            .iter()
            .filter_map(|s| crate::Language::parse(s))
            .collect()
    }

    /// Whether the `code` tool should be registered at all.
    ///
    /// An empty language list means "no code execution", not "all languages".
    /// The registry builder used to read it the other way round.
    pub fn code_enabled(&self) -> bool {
        !self.code_languages().is_empty()
    }

    pub fn system_tool(&self) -> crate::SystemTool {
        crate::SystemTool::new()
            .with_allowed_env(self.system.allowed_env.clone())
            .with_process_list(self.system.allow_process_list)
            .with_kill(self.system.allow_kill)
            .with_max_sleep_ms(self.system.max_sleep_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_is_valid() {
        ExecutionPolicy::default().validate().unwrap();
    }

    #[test]
    fn yaml_roundtrip_minimal() {
        let yaml = r#"
workspace: /tmp/test_exec
concurrency: 8
filesystem:
  writable: true
shell:
  commands:
    - program: /bin/echo
      args: NoFlags
http:
  allowed_hosts: [api.github.com]
"#;
        let p = ExecutionPolicy::from_yaml(yaml).unwrap();
        assert_eq!(p.concurrency, 8);
        assert_eq!(p.http.allowed_hosts, vec!["api.github.com"]);
        assert_eq!(p.shell.commands.len(), 1);
    }

    #[test]
    fn yaml_exact_args() {
        let yaml = r#"
shell:
  commands:
    - program: /usr/bin/git
      args:
        Exact: [["status"], ["log", "--oneline"]]
"#;
        let p = ExecutionPolicy::from_yaml(yaml).unwrap();
        let cmds = p.allowed_commands();
        assert!(matches!(cmds[0].arguments, crate::ArgumentPolicy::Exact(_)));
    }

    #[test]
    fn rejects_relative_program() {
        let yaml = r#"shell: { commands: [{program: echo, args: None}]}"#;
        assert!(ExecutionPolicy::from_yaml(yaml).is_err());
    }

    #[test]
    fn rejects_wildcard_host() {
        let yaml = r#"http: { allowed_hosts: ["*.evil.com"] }"#;
        assert!(ExecutionPolicy::from_yaml(yaml).is_err());
    }

    #[test]
    fn system_policy_parses_and_builds_tool() {
        use crate::Tool;
        let yaml = r#"
system:
  allowed_env: [PATH, TZ]
  allow_process_list: true
  allow_kill: false
  max_sleep_ms: 2000
"#;
        let p = ExecutionPolicy::from_yaml(yaml).unwrap();
        assert_eq!(p.system.allowed_env, vec!["PATH", "TZ"]);
        assert!(p.system.allow_process_list);
        let tool = p.system_tool();
        assert!(tool.parameters_schema().get("properties").is_some());
    }

    #[test]
    fn code_is_denied_by_default() {
        // The default used to enable python/bash/javascript, making an
        // unconfigured daemon more permissive than a configured one.
        let p = ExecutionPolicy::default();
        assert!(p.code.allowed_languages.is_empty());
        assert!(!p.code_enabled());
        assert!(!p.code.allow_unsandboxed);
    }

    #[test]
    fn an_empty_language_list_means_no_languages() {
        // Not "all languages" — which is how the registry builder read it.
        let p = ExecutionPolicy::from_yaml("code: { allowed_languages: [] }").unwrap();
        assert!(p.code_languages().is_empty());
        assert!(!p.code_enabled());
    }

    #[test]
    fn enabling_a_language_requires_acknowledging_it_is_unsandboxed() {
        let yaml = r#"code: { allowed_languages: [python] }"#;
        let err = ExecutionPolicy::from_yaml(yaml).unwrap_err().to_string();
        assert!(err.contains("allow_unsandboxed"), "{err}");

        let yaml = r#"code: { allowed_languages: [python], allow_unsandboxed: true }"#;
        let p = ExecutionPolicy::from_yaml(yaml).unwrap();
        assert!(p.code_enabled());
        assert_eq!(p.code_languages().len(), 1);
    }

    #[test]
    fn rejects_bad_system_policy() {
        let yaml = r#"system: { max_sleep_ms: 99999 }"#;
        assert!(ExecutionPolicy::from_yaml(yaml).is_err());
        let yaml = r#"system: { allowed_env: ["HAS SPACE"] }"#;
        assert!(ExecutionPolicy::from_yaml(yaml).is_err());
        let yaml = r#"system: { allowed_env: ["123BAD"] }"#;
        assert!(ExecutionPolicy::from_yaml(yaml).is_err());
    }
}
