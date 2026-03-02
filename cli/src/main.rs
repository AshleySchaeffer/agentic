use clap::{Parser, Subcommand};
use memchr::memmem;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::{env, fs, io, process};

static DEBUG: LazyLock<bool> =
    LazyLock::new(|| env::var("AGENTIC_DEBUG").is_ok_and(|v| !v.is_empty()));

macro_rules! debug {
    ($($arg:tt)*) => {
        if *DEBUG {
            eprintln!("[hook] {}", format!($($arg)*));
        }
    };
}

const SIZE_THRESHOLD: usize = 4096;
const CODE_HEAVY_THRESHOLD: usize = 2048;

// ── Embedded content ─────────────────────────────────────────────────

const AGENTS: &[(&str, &str)] = &[
    ("dev.md", include_str!("../agents/dev.md")),
    ("reviewer.md", include_str!("../agents/reviewer.md")),
];

// Files from prior installs; cleaned up on install/uninstall.
const LEGACY_REFS: &[&str] = &[
    "analyst.md",
    "architect.md",
    "auditor.md",
    "challenger.md",
    "qa.md",
    "sign-off-protocol.md",
    "worker-protocol.md",
    "orchestration-rules.md",
];

const GLOBAL_CLAUDE_MD: &str = include_str!("../CLAUDE.md");
const CODING_STANDARDS_MD: &str = include_str!("../coding-standards.md");
const PLANNING_PROTOCOL: &str = include_str!("../planning-protocol.md");

// ── CLI ──────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "agentic")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Install agentic workflow globally
    Install,
    /// Remove agentic workflow (agents, hooks, binary)
    Uninstall,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Install) => install(),
        Some(Command::Uninstall) => uninstall(),
        None => hook_dispatch(),
    }
}

// ── Hook dispatch ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct HookInput {
    hook_event_name: String,
    tool_name: Option<String>,
    tool_input: Option<Value>,
    cwd: String,
}

fn hook_dispatch() {
    let mut input = String::new();
    if io::Read::read_to_string(&mut io::stdin(), &mut input).is_err() {
        process::exit(1);
    }

    let hook: HookInput = match serde_json::from_str(&input) {
        Ok(h) => h,
        Err(_) => process::exit(1),
    };

    let tool = hook.tool_name.as_deref().unwrap_or("");

    let handler = match (hook.hook_event_name.as_str(), tool) {
        ("PreToolUse", "SendMessage") => Some("message_transform"),
        ("PreToolUse", "Agent") => Some("spawn_mode"),
        ("PreToolUse", "EnterPlanMode") => Some("planning_protocol"),
        ("PostToolUse", "Bash") => Some("post_merge"),
        _ => None,
    };

    debug!(
        "{} {}{}",
        hook.hook_event_name,
        tool,
        match handler {
            Some(h) => format!(" → {h}"),
            None => " → no-op".into(),
        }
    );

    match handler {
        Some("message_transform") => message_transform(&hook),
        Some("spawn_mode") => spawn_mode(&hook),
        Some("planning_protocol") => planning_protocol(),
        Some("post_merge") => post_merge(&hook),
        _ => {}
    }
}

/// Hook 1: Message Transformation
/// Intercepts large SendMessage payloads, writes content to disk,
/// replaces the message body with a file reference.
fn message_transform(hook: &HookInput) {
    let input = match &hook.tool_input {
        Some(v) => v,
        None => return,
    };

    let content = match input.get("content").and_then(Value::as_str) {
        Some(s) => s,
        None => return,
    };

    let bytes = content.as_bytes();
    let code_fences = memmem::find_iter(bytes, b"```").count();
    let threshold = if code_fences >= 2 {
        CODE_HEAVY_THRESHOLD
    } else {
        SIZE_THRESHOLD
    };

    if bytes.len() <= threshold {
        debug!("skip: {} bytes ≤ {threshold} threshold", bytes.len());
        return;
    }

    let dir = Path::new(&hook.cwd).join(".claude/messages");
    if fs::create_dir_all(&dir).is_err() {
        return;
    }

    let hash = Sha256::digest(bytes);
    let name: String = hash.iter().take(8).map(|b| format!("{b:02x}")).collect();
    let path = dir.join(format!("{name}.md"));

    if fs::write(&path, content).is_err() {
        return;
    }

    debug!("offloaded {} bytes → {}", bytes.len(), path.display());

    let summary = input
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or("(see file)");
    let mut updated = input.clone();
    updated["content"] = Value::String(format!(
        "[Content offloaded to {} ({} bytes). Summary: {summary}]",
        path.display(),
        bytes.len(),
    ));

    let output = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "updatedInput": updated
        }
    });
    serde_json::to_writer(io::stdout(), &output).ok();
}

/// Hook 2: Agent Spawn Mode Enforcement
/// Forces all agent spawns to use mode: "acceptEdits".
fn spawn_mode(hook: &HookInput) {
    let input = match &hook.tool_input {
        Some(v) => v,
        None => return,
    };

    let current = input
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("default");
    if current == "acceptEdits" {
        debug!("mode already acceptEdits");
        return;
    }

    debug!("mode {current} → acceptEdits");

    let mut updated = input.clone();
    updated["mode"] = Value::String("acceptEdits".into());

    let output = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "updatedInput": updated
        }
    });
    serde_json::to_writer(io::stdout(), &output).ok();
}

/// Hook 4: Adaptive Planning Protocol
/// Injects the full planning protocol as additionalContext when plan mode is entered.
fn planning_protocol() {
    debug!("injecting planning protocol");

    let output = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "additionalContext": PLANNING_PROTOCOL
        }
    });
    serde_json::to_writer(io::stdout(), &output).ok();
}

/// Hook 3: Post-Merge Project Refresh
/// After a git merge, injects context telling the agent to check project-config.md.
fn post_merge(hook: &HookInput) {
    let command = hook
        .tool_input
        .as_ref()
        .and_then(|v| v.get("command"))
        .and_then(Value::as_str)
        .unwrap_or("");

    if !command.contains("git merge") && !command.contains("git pull") {
        debug!("not a merge command");
        return;
    }

    debug!("merge detected: {command}");

    let output = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "additionalContext": "A merge just landed. Check if project-config.md is still accurate and update it if needed."
        }
    });
    serde_json::to_writer(io::stdout(), &output).ok();
}

// ── Install & Uninstall ──────────────────────────────────────────────

fn home_dir() -> PathBuf {
    PathBuf::from(env::var("HOME").expect("HOME not set"))
}

fn write_file(content: &str, dst: &Path) {
    fs::write(dst, content).unwrap_or_else(|e| {
        eprintln!("Failed to write {}: {e}", dst.display());
        process::exit(1);
    });
    println!("ok    {}", dst.display());
}

fn install() {
    let home = home_dir();
    let claude_dir = home.join(".claude");

    // Agents
    let agents_dir = claude_dir.join("agents");
    fs::create_dir_all(&agents_dir).unwrap();
    for (name, content) in AGENTS {
        write_file(content, &agents_dir.join(name));
    }

    // Remove files from prior installs
    for name in LEGACY_REFS {
        let path = claude_dir.join("agents").join(name);
        if path.exists() {
            let _ = fs::remove_file(&path);
            println!("rm    {} (legacy)", path.display());
        }
    }
    // Also clean standalone legacy files from the claude dir root
    for name in &["sign-off-protocol.md", "worker-protocol.md", "orchestration-rules.md"] {
        let path = claude_dir.join(name);
        if path.exists() {
            let _ = fs::remove_file(&path);
            println!("rm    {} (legacy)", path.display());
        }
    }

    // Global CLAUDE.md + coding-standards.md
    write_file(GLOBAL_CLAUDE_MD, &claude_dir.join("CLAUDE.md"));
    write_file(CODING_STANDARDS_MD, &claude_dir.join("coding-standards.md"));

    // Install binary
    let bin_dir = home.join(".local/bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let exe = env::current_exe().unwrap();
    let bin_dst = bin_dir.join("agentic");
    if exe != bin_dst {
        fs::copy(&exe, &bin_dst).unwrap_or_else(|e| {
            eprintln!("Failed to install binary: {e}");
            process::exit(1);
        });
        println!("ok    {}", bin_dst.display());
    }

    // Clean up old agentic-hooks binary
    let old_bin = bin_dir.join("agentic-hooks");
    if old_bin.exists() {
        let _ = fs::remove_file(&old_bin);
        println!("rm    {} (renamed)", old_bin.display());
    }

    // Configure settings.json
    let settings_path = claude_dir.join("settings.json");
    let mut settings: Value = fs::read_to_string(&settings_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    let obj = settings.as_object_mut().unwrap();

    obj.entry("env")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .unwrap()
        .insert(
            "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS".into(),
            Value::String("1".into()),
        );

    obj.insert(
        "hooks".into(),
        serde_json::json!({
            "PreToolUse": [
                { "matcher": "SendMessage", "hooks": [{ "type": "command", "command": "agentic" }] },
                { "matcher": "Agent", "hooks": [{ "type": "command", "command": "agentic" }] },
                { "matcher": "EnterPlanMode", "hooks": [{ "type": "command", "command": "agentic" }] }
            ],
            "PostToolUse": [
                { "matcher": "Bash", "hooks": [{ "type": "command", "command": "agentic" }] }
            ]
        }),
    );

    fs::write(
        &settings_path,
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .unwrap();
    println!("ok    {}", settings_path.display());

    println!("\nInstall complete. Restart Claude Code for changes to take effect.");
}

fn uninstall() {
    let home = home_dir();
    let claude_dir = home.join(".claude");

    for (name, _) in AGENTS {
        let path = claude_dir.join("agents").join(name);
        if path.exists() {
            let _ = fs::remove_file(&path);
            println!("rm    {}", path.display());
        }
    }

    for name in LEGACY_REFS {
        let path = claude_dir.join("agents").join(name);
        if path.exists() {
            let _ = fs::remove_file(&path);
            println!("rm    {}", path.display());
        }
    }
    for name in &["sign-off-protocol.md", "worker-protocol.md", "orchestration-rules.md"] {
        let path = claude_dir.join(name);
        if path.exists() {
            let _ = fs::remove_file(&path);
            println!("rm    {}", path.display());
        }
    }

    // Remove coding-standards.md
    let coding_standards = claude_dir.join("coding-standards.md");
    if coding_standards.exists() {
        let _ = fs::remove_file(&coding_standards);
        println!("rm    {}", coding_standards.display());
    }

    let settings_path = claude_dir.join("settings.json");
    if settings_path.exists()
        && let Ok(content) = fs::read_to_string(&settings_path)
        && let Ok(mut settings) = serde_json::from_str::<Value>(&content)
    {
        if let Some(obj) = settings.as_object_mut() {
            obj.remove("hooks");
            if let Some(env_obj) = obj.get_mut("env").and_then(Value::as_object_mut) {
                env_obj.remove("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS");
            }
        }
        let _ = fs::write(
            &settings_path,
            serde_json::to_string_pretty(&settings).unwrap(),
        );
        println!("ok    {} (hooks removed)", settings_path.display());
    }

    // Remove both current and old binary names
    for name in &["agentic", "agentic-hooks"] {
        let bin = home.join(".local/bin").join(name);
        if bin.exists() {
            let _ = fs::remove_file(&bin);
            println!("rm    {}", bin.display());
        }
    }

    println!("\nUninstall complete.");
}

