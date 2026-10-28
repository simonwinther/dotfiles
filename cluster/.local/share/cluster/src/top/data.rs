use anyhow::{Context, Result, bail};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read},
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

#[cfg(not(target_os = "linux"))]
compile_error!("The monitor's subprocess capture currently requires Linux");

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Node {
    pub name: String,
    pub state: String,
    pub partitions: Vec<String>,
    pub models: BTreeMap<String, u32>,
    pub gpu_used: u32,
    pub gpu_known: bool,
    pub cpu_used: u32,
    pub cpu_total: u32,
    pub ram_used: u64,
    pub ram_total: u64,
    pub reason: String,
}
impl Node {
    pub fn total(&self) -> u32 {
        self.models.values().sum()
    }
    pub fn unavailable(&self) -> bool {
        let s = self.state.to_lowercase();
        [
            "down",
            "drain",
            "fail",
            "maint",
            "unknown",
            "resv",
            "not_responding",
            "power_down",
            "powered_down",
        ]
        .iter()
        .any(|x| s.contains(x))
            || s.contains('*')
    }
    pub fn free(&self) -> u32 {
        if self.unavailable() || !self.gpu_known {
            0
        } else {
            self.total().saturating_sub(self.gpu_used)
        }
    }
    pub fn category(&self) -> &'static str {
        if self.unavailable() {
            "offline"
        } else if !self.gpu_known {
            "unknown"
        } else if self.free() == self.total() {
            "idle"
        } else if self.free() > 0 {
            "mixed"
        } else {
            "full"
        }
    }
    pub fn cpu_free(&self) -> u32 {
        if self.unavailable() {
            0
        } else {
            self.cpu_total.saturating_sub(self.cpu_used)
        }
    }
    pub fn models_label(&self) -> String {
        self.models
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join("+")
            .to_uppercase()
    }
}
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Job {
    pub id: String,
    pub user: String,
    pub state: String,
    pub partition: String,
    pub priority: u64,
    pub submitted: String,
    pub elapsed: String,
    pub limit: String,
    pub gres: String,
    pub reason: String,
    pub name: String,
    pub nodes: String,
    pub end: String,
    pub exit_code: String,
    pub work_dir: String,
    pub start: String,
    pub eligible: String,
    pub cpus: String,
    pub req_mem: String,
    pub account: String,
    pub qos: String,
    pub req_tres: String,
}
impl Job {
    pub fn pending(&self) -> bool {
        self.state == "PENDING"
    }
    pub fn running(&self) -> bool {
        self.state == "RUNNING"
    }
    pub fn matches_gpu(&self, filter: &str) -> bool {
        if filter.is_empty() {
            return true;
        }
        let models = gpu_counts(&self.gres);
        models
            .keys()
            .any(|m| m == "gpu" || m.contains(&filter.to_lowercase()))
    }
}
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    pub nodes: Vec<Node>,
    pub jobs: Vec<Job>,
    #[serde(skip)]
    pub fetched: Option<Instant>,
}

const STDOUT_LIMIT: usize = 16 * 1024 * 1024;
const STDERR_LIMIT: usize = 1024 * 1024;
const READ_BUDGET: usize = 64 * 1024;

// Linux's fcntl constants. Keep pipe reads in the polling thread: reader threads
// can outlive a timeout when a descendant inherits stdout or stderr.
const F_GETFL: i32 = 3;
const F_SETFL: i32 = 4;
const O_NONBLOCK: i32 = 0o4000;
const SIGKILL: i32 = 9;
unsafe extern "C" {
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
}

struct CommandRegistry {
    shutting_down: bool,
    groups: BTreeSet<u32>,
}
static COMMANDS: Mutex<CommandRegistry> = Mutex::new(CommandRegistry {
    shutting_down: false,
    groups: BTreeSet::new(),
});

// Call before main returns: process exit does not run background threads' Drop
// implementations. The spawn lock also prevents a late worker from launching a
// new command after shutdown has begun. Killing never waits while holding it.
pub fn shutdown_commands() {
    let mut registry = COMMANDS.lock().unwrap_or_else(|error| error.into_inner());
    registry.shutting_down = true;
    for group in std::mem::take(&mut registry.groups) {
        kill_group(group);
    }
}

fn kill_group(group: u32) {
    // SAFETY: registered children use process_group(0), giving each its own
    // process group. A negative PID addresses that group and its descendants.
    unsafe { kill(-(group as i32), SIGKILL) };
}

struct RunningCommand {
    child: Child,
    group_stopped: bool,
}
impl RunningCommand {
    fn stop_group(&mut self) {
        if self.group_stopped {
            return;
        }
        let mut registry = COMMANDS.lock().unwrap_or_else(|error| error.into_inner());
        if registry.groups.remove(&self.child.id()) {
            kill_group(self.child.id());
        }
        self.group_stopped = true;
    }
}
impl Drop for RunningCommand {
    fn drop(&mut self) {
        self.stop_group();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn nonblocking(pipe: &impl AsRawFd) -> io::Result<()> {
    // SAFETY: the pipe owns this valid descriptor throughout both calls.
    let flags = unsafe { fcntl(pipe.as_raw_fd(), F_GETFL) };
    if flags < 0 || unsafe { fcntl(pipe.as_raw_fd(), F_SETFL, flags | O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

// Returns true when more buffered data may remain. Each pass is bounded so a
// continuously writing process cannot keep us from checking its deadline.
fn drain_pipe(pipe: &mut impl Read, output: &mut Vec<u8>, limit: usize) -> Result<bool> {
    let mut buffer = [0u8; 8192];
    let mut read = 0;
    while read < READ_BUDGET {
        match pipe.read(&mut buffer) {
            Ok(0) => return Ok(false),
            Ok(n) => {
                if n > limit.saturating_sub(output.len()) {
                    bail!("output exceeded the {} KiB capture limit", limit / 1024);
                }
                output.extend_from_slice(&buffer[..n]);
                read += n;
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => return Ok(true),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(true)
}

// Capture both streams without blocking or unbounded allocation. Commands and
// their process groups are stopped on every exit path; invalid UTF-8 in job
// names or diagnostic output is displayed with replacement characters.
pub fn command(program: &str, args: &[&str], timeout: Duration) -> Result<String> {
    command_with_limits(program, args, timeout, STDOUT_LIMIT, STDERR_LIMIT)
}

pub(crate) fn command_with_limits(
    program: &str,
    args: &[&str],
    timeout: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
) -> Result<String> {
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null());
    capture(&mut command, timeout, stdout_limit, stderr_limit)?.text(program)
}

pub(crate) struct Capture {
    pub status: std::process::ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
impl Capture {
    pub fn text(self, program: &str) -> Result<String> {
        if !self.status.success() {
            let error = String::from_utf8_lossy(&self.stderr);
            if error.trim().is_empty() {
                bail!("{program}: {}", self.status);
            }
            bail!("{program}: {}", error.trim());
        }
        Ok(String::from_utf8_lossy(&self.stdout).into_owned())
    }
}
pub(crate) fn capture(
    command: &mut Command,
    timeout: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
) -> Result<Capture> {
    let program = command.get_program().to_string_lossy().into_owned();
    let start = Instant::now();
    let mut registry = COMMANDS.lock().unwrap_or_else(|error| error.into_inner());
    if registry.shutting_down {
        bail!("Cannot start {program}: monitor is shutting down");
    }
    let child = command
        .env("LC_ALL", "C")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .with_context(|| format!("Cannot start {program}; check your PATH"))?;
    registry.groups.insert(child.id());
    drop(registry);
    let mut child = RunningCommand {
        child,
        group_stopped: false,
    };
    let mut stdout = child
        .child
        .stdout
        .take()
        .context("Missing command stdout pipe")?;
    let mut stderr = child
        .child
        .stderr
        .take()
        .context("Missing command stderr pipe")?;
    nonblocking(&stdout).context("Cannot configure command stdout")?;
    nonblocking(&stderr).context("Cannot configure command stderr")?;
    let mut output = Vec::new();
    let mut error = Vec::new();
    let status = loop {
        let status = child
            .child
            .try_wait()
            .with_context(|| format!("Cannot wait for {program}"))?;
        if status.is_some() {
            child.stop_group();
        }
        let out_pending = drain_pipe(&mut stdout, &mut output, stdout_limit)
            .map_err(|error| anyhow::anyhow!("{program} stdout: {error}"))?;
        let err_pending = drain_pipe(&mut stderr, &mut error, stderr_limit)
            .map_err(|error| anyhow::anyhow!("{program} stderr: {error}"))?;
        // The direct child has finished writing. Drain its buffered bytes, then
        // close the pipes even if an escaped descendant still holds them open.
        if let Some(status) = status
            && !out_pending
            && !err_pending
        {
            break status;
        }
        if start.elapsed() >= timeout {
            bail!("{program} timed out after {:.1}s", timeout.as_secs_f64());
        }
        if !out_pending && !err_pending {
            thread::sleep(Duration::from_millis(10).min(timeout.saturating_sub(start.elapsed())));
        }
    };
    Ok(Capture {
        status,
        stdout: output,
        stderr: error,
    })
}

fn number(s: &str) -> u32 {
    s.parse().unwrap_or(0)
}
pub fn memory(s: &str) -> u64 {
    let split = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(s.len());
    let n = s[..split].parse::<f64>().unwrap_or(0.0);
    let mult = match s[split..]
        .chars()
        .next()
        .unwrap_or('M')
        .to_ascii_uppercase()
    {
        'K' => 1.0 / 1024.0,
        'G' => 1024.0,
        'T' => 1024.0 * 1024.0,
        _ => 1.0,
    };
    (n * mult) as u64
}
// Parenthesized socket/index lists contain commas; they are not GRES separators.
pub fn gpu_counts(s: &str) -> BTreeMap<String, u32> {
    let mut clean = String::new();
    let mut depth = 0u32;
    for c in s.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => clean.push(c),
            _ => {}
        }
    }
    let mut result = BTreeMap::new();
    let mut generic = 0;
    for item in clean.split(',') {
        if let Some((key, value)) = item.split_once('=') {
            if key == "gres/gpu" {
                generic = number(value);
            } else if let Some(model) = key.strip_prefix("gres/gpu:") {
                *result.entry(model.to_lowercase()).or_default() += number(value);
            }
        } else {
            let fields: Vec<_> = item.split(':').collect();
            if fields.first() != Some(&"gpu") {
                continue;
            }
            let (model, count) = match fields.as_slice() {
                [_, n] if n.parse::<u32>().is_ok() => ("gpu", number(n)),
                [_, model, n, ..] => (*model, number(n)),
                [_, model] => (*model, 1),
                _ => ("gpu", 1),
            };
            *result.entry(model.to_lowercase()).or_default() += count;
        }
    }
    let typed: u32 = result.values().sum();
    if generic > typed {
        result.insert("gpu".into(), generic - typed);
    }
    result.retain(|_, n| *n > 0);
    result
}
fn fields(record: &str) -> BTreeMap<&str, &str> {
    record
        .split_whitespace()
        .filter_map(|item| item.split_once('='))
        .collect()
}
pub fn parse_nodes(text: &str, partition: &str) -> Vec<Node> {
    text.split("NodeName=")
        .skip(1)
        .filter_map(|record| {
            let name = record.split_whitespace().next()?.to_string();
            let f = fields(record);
            let get = |key| f.get(key).copied().unwrap_or("");
            let partitions: Vec<_> = get("Partitions").split(',').map(str::to_string).collect();
            if !partition.is_empty()
                && !partition
                    .split(',')
                    .any(|p| partitions.iter().any(|x| x == p))
            {
                return None;
            }
            let mut models = gpu_counts(get("Gres"));
            if models.is_empty() || models.contains_key("gpu") {
                let cfg = gpu_counts(get("CfgTRES"));
                if !cfg.is_empty() {
                    models = cfg;
                }
            }
            let gpu_used = gpu_counts(get("AllocTRES")).values().sum();
            let reason = record
                .split_once("Reason=")
                .map(|(_, r)| r.split(" Comment=").next().unwrap_or(r).trim())
                .unwrap_or("");
            Some(Node {
                name,
                partitions,
                models,
                gpu_used,
                gpu_known: f.contains_key("AllocTRES"),
                state: get("State").into(),
                cpu_used: number(get("CPUAlloc")),
                cpu_total: number(get("CPUTot")),
                ram_used: memory(get("AllocMem")),
                ram_total: memory(get("RealMemory")),
                reason: reason.into(),
            })
        })
        .collect()
}
pub fn parse_jobs(text: &str) -> Result<Vec<Job>> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(line_index, line)| {
            let p: Vec<_> = line.split('\x1f').map(str::trim).collect();
            if p.len() != 12 || p[0].is_empty() || p[1].is_empty() || p[2].is_empty() {
                bail!("Unrecognized squeue response at line {}", line_index + 1);
            }
            let priority = p[4]
                .parse()
                .with_context(|| format!("Invalid squeue priority at line {}", line_index + 1))?;
            Ok(Job {
                id: p[0].into(),
                user: p[1].into(),
                state: p[2].into(),
                partition: p[3].into(),
                priority,
                submitted: p[5].into(),
                elapsed: p[6].into(),
                limit: p[7].into(),
                gres: p[8].into(),
                reason: p[9].into(),
                name: p[10].into(),
                nodes: p[11].into(),
                ..Default::default()
            })
        })
        .collect()
}
pub fn fetch(partition: &str, timeout: Duration) -> Result<Snapshot> {
    let nodes = command("scontrol", &["show", "nodes", "-o"], timeout)?;
    let fmt = [
        "%i", "%u", "%T", "%P", "%Q", "%V", "%M", "%l", "%b", "%R", "%j", "%N",
    ]
    .join("\x1f");
    let jobs = command("squeue", &["-a", "-h", "-o", &fmt], timeout)?;
    let parsed_nodes = parse_nodes(&nodes, partition);
    if !nodes.trim().is_empty() && !nodes.contains("NodeName=") {
        bail!("Unrecognized scontrol response");
    }
    let parsed_jobs = parse_jobs(&jobs)?;
    Ok(Snapshot {
        nodes: parsed_nodes,
        jobs: parsed_jobs,
        fetched: Some(Instant::now()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gres_and_tres_do_not_double_count() {
        assert_eq!(
            gpu_counts("gres/gpu=4,gres/gpu:a100=4")
                .values()
                .sum::<u32>(),
            4
        );
        assert_eq!(
            gpu_counts("gpu:a100:4(S:0,1),gpu:h100:2")
                .values()
                .sum::<u32>(),
            6
        );
        assert_eq!(gpu_counts("gpu:8").get("gpu"), Some(&8));
    }
    #[test]
    fn partition_state_and_empty_allocations() {
        let text = "NodeName=a State=IDLE CPUAlloc=0 CPUTot=64 Partitions=gpu,preempt Gres=gpu:a100:4 AllocTRES= RealMemory=64000 AllocMem=0\nNodeName=b State=DOWN Partitions=private Gres=gpu:h100:8 AllocTRES=\nNodeName=c State=IDLE+DRAIN Partitions=gpu Gres=gpu:a100:4 AllocTRES=gres/gpu=2,gres/gpu:a100=2 Reason=maintenance window Comment=(null)";
        let n = parse_nodes(text, "gpu");
        assert_eq!(n.len(), 2);
        assert_eq!(n[0].free(), 4);
        assert_eq!(n[1].free(), 0);
        assert_eq!(n[1].gpu_used, 2);
        assert_eq!(n[1].reason, "maintenance window");
    }
    #[test]
    fn missing_alloc_is_unknown() {
        assert!(!parse_nodes("NodeName=a Partitions=gpu Gres=gpu:4", "gpu")[0].gpu_known);
    }
    #[test]
    fn queue_separator_and_arrays() {
        let s = [
            "123_[1-20]",
            "me",
            "PENDING",
            "gpu",
            "123",
            "2026-10-15",
            "0:00",
            "1:00",
            "gpu:a100:1",
            "Resources",
            "name with | pipe",
            "",
        ]
        .join("\x1f");
        let j = parse_jobs(&s).unwrap();
        assert_eq!(j.len(), 1);
        assert_eq!(j[0].name, "name with | pipe");
        assert!(j[0].matches_gpu("a100"));
    }
    #[test]
    fn queue_rejects_partial_or_invalid_responses() {
        let valid = "123\x1fme\x1fRUNNING\x1fgpu\x1f42\x1ftoday\x1f0:01\x1f1:00\x1fgpu:1\x1fnode\x1fjob\x1fnode";
        assert_eq!(parse_jobs(&format!("\n{valid}\n\n")).unwrap().len(), 1);
        assert!(parse_jobs(&format!("{valid}\nunexpected warning")).is_err());
        assert!(parse_jobs(&valid.replace("\x1f42\x1f", "\x1funknown\x1f")).is_err());
        assert!(parse_jobs("  \n").unwrap().is_empty());
    }
    #[test]
    fn captures_large_stderr_and_invalid_utf8() {
        let output = command(
            "sh",
            &["-c", "head -c 131072 /dev/zero >&2; printf '\\377done'"],
            Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(output, "\u{fffd}done");
        let error = command("sh", &["-c", "exit 7"], Duration::from_secs(1))
            .unwrap_err()
            .to_string();
        assert!(error.contains('7'), "{error}");
    }
    #[test]
    fn capture_limits_stop_continuous_writers() {
        for script in [
            "while :; do printf '0123456789abcdef'; done",
            "while :; do printf '0123456789abcdef' >&2; done",
        ] {
            let start = Instant::now();
            let error =
                command_with_limits("sh", &["-c", script], Duration::from_secs(2), 1024, 1024)
                    .unwrap_err()
                    .to_string();
            assert!(error.contains("capture limit"), "{error}");
            assert!(start.elapsed() < Duration::from_secs(1));
        }
    }
    #[test]
    fn closes_inherited_pipes_and_stops_descendants() {
        let marker = std::env::temp_dir().join(format!(
            "mytop-descendant-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let marker_path = marker.to_str().unwrap();
        let script = "(sleep 0.2; printf leaked > \"$1\") & printf done";
        let start = Instant::now();
        assert_eq!(
            command(
                "sh",
                &["-c", script, "sh", marker_path],
                Duration::from_secs(2)
            )
            .unwrap(),
            "done"
        );
        assert!(start.elapsed() < Duration::from_secs(1));
        // A successful leader exit must not leave its background process alive.
        thread::sleep(Duration::from_millis(300));
        assert!(!marker.exists(), "a descendant survived its parent command");

        let timeout_script = "(sleep 0.2; printf leaked > \"$1\") & wait";
        assert!(
            command(
                "sh",
                &["-c", timeout_script, "sh", marker_path],
                Duration::from_millis(30),
            )
            .is_err()
        );
        thread::sleep(Duration::from_millis(300));
        assert!(
            !marker.exists(),
            "a descendant survived the command timeout"
        );
    }
    #[test]
    fn shutdown_cancels_active_commands_and_prevents_late_spawns() {
        // Shutdown is permanent. Exercise it in an isolated copy of this test
        // process so parallel tests retain their own live command registry.
        const FIXTURE: &str = "MYTOP_COMMAND_SHUTDOWN_TEST";
        if std::env::var_os(FIXTURE).is_none() {
            let module = module_path!().split_once("::").unwrap().1;
            let name =
                format!("{module}::shutdown_cancels_active_commands_and_prevents_late_spawns");
            let result = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", &name, "--nocapture"])
                .env(FIXTURE, "1")
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&result.stdout);
            let stderr = String::from_utf8_lossy(&result.stderr);
            assert!(
                result.status.success() && stdout.contains("1 passed"),
                "{stdout}{stderr}"
            );
            return;
        }
        let running = thread::spawn(|| command("sleep", &["10"], Duration::from_secs(20)));
        let start = Instant::now();
        loop {
            if !COMMANDS.lock().unwrap().groups.is_empty() {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "command failed to start"
            );
            thread::sleep(Duration::from_millis(5));
        }
        shutdown_commands();
        assert!(running.join().unwrap().is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
        let error = command("sh", &["-c", "exit 0"], Duration::from_secs(1)).unwrap_err();
        assert!(error.to_string().contains("shutting down"));
    }
    #[test]
    fn failure_and_timeout() {
        assert!(
            command(
                "sh",
                &["-c", "echo broken >&2; exit 1"],
                Duration::from_secs(1)
            )
            .unwrap_err()
            .to_string()
            .contains("broken")
        );
        let start = Instant::now();
        assert!(command("sleep", &["2"], Duration::from_millis(30)).is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
