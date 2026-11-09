//! Docker commands for worker containers, as data.
//!
//! The builders here return argument lists, so the security settings can be checked by
//! tests without a Docker daemon. `CliDockerRunner` in `lib.rs` runs them.
//!
//! Isolation model:
//!
//! - Each task gets its own internal network. An internal network has no route to the
//!   internet and no route to the host.
//! - A relay container is attached to every task network under the alias
//!   `frontal-relay`. It forwards one TCP port to the server and nothing else. It is
//!   the only way out for a worker, and it cannot be used as a general proxy.
//! - Model traffic goes through the server's provider proxy, so the worker does not
//!   need the internet for model calls.
//! - The container runs with no Linux capabilities, no privilege escalation, a read-only
//!   root filesystem, bounded memory, CPU, and process count, and writable space only in
//!   the task checkout and two tmpfs mounts.

/// Container name of the relay that serves `port`. One relay per server port, so two
/// servers with different ports on one Docker host do not replace each other's relay.
pub(crate) fn relay_container_name(port: u16) -> String {
    format!("frontal-code-relay-{port}")
}

/// Network alias the relay answers to inside a task network. Workers use this host name.
pub(crate) const RELAY_ALIAS: &str = "frontal-relay";

/// Label that marks task networks, so idle ones can be found and removed.
pub(crate) const TASK_NETWORK_LABEL: &str = "frontal-code.task_network";

/// Label on a task network that records the server port that created it. A sweep only
/// touches networks of its own server.
pub(crate) const NETWORK_RELAY_PORT_LABEL: &str = "frontal-code.relay_port";

/// Label on a task network with its creation time in Unix seconds. A sweep leaves young
/// networks alone, because a launch may be between creating the network and starting its
/// worker.
pub(crate) const NETWORK_CREATED_LABEL: &str = "frontal-code.created_at";

/// Label on worker containers that names their network, so `stop` can clean it up.
pub(crate) const TASK_NETWORK_CONTAINER_LABEL: &str = "frontal-code.task_network";

/// Label on the relay that records the port it forwards, so a changed port recreates it.
pub(crate) const RELAY_PORT_LABEL: &str = "frontal-code.relay_port";

/// Writable tmpfs for the worker's temporary files.
const TMP_TMPFS: &str = "/tmp:rw,nosuid,nodev,size=512m";

/// Writable tmpfs for the worker user's home directory.
const HOME_TMPFS: &str = "/home/frontal-code:rw,nosuid,nodev,size=1g";

/// Name of a task's internal network. Task ids are sanitized to Docker's name rules.
pub(crate) fn task_network_name(task_id: &str) -> String {
    let safe: String = task_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(48)
        .collect();
    format!("frontal-task-{safe}")
}

/// Arguments for starting the relay. It forwards `port` on the relay to the same port on
/// the host, where the server listens.
pub(crate) fn relay_run_args(image: &str, port: u16) -> Vec<String> {
    vec![
        "run".to_string(),
        "-d".to_string(),
        "--name".to_string(),
        relay_container_name(port),
        "--restart".to_string(),
        "unless-stopped".to_string(),
        "--add-host".to_string(),
        "host.docker.internal:host-gateway".to_string(),
        "--cap-drop".to_string(),
        "ALL".to_string(),
        "--security-opt".to_string(),
        "no-new-privileges:true".to_string(),
        "--read-only".to_string(),
        "--pids-limit".to_string(),
        "64".to_string(),
        "--memory".to_string(),
        "128m".to_string(),
        "--label".to_string(),
        "frontal-code.role=relay".to_string(),
        "--label".to_string(),
        format!("frontal-code.relay_port={port}"),
        image.to_string(),
        format!("TCP-LISTEN:{port},fork,reuseaddr"),
        format!("TCP4:host.docker.internal:{port}"),
    ]
}

/// Security flags for a worker container. Every one is deliberate.
pub(crate) fn worker_hardening_args() -> Vec<String> {
    [
        // No Linux capabilities. The worker runs as an unprivileged user.
        "--cap-drop",
        "ALL",
        // No setuid or file-capability escalation inside the container.
        "--security-opt",
        "no-new-privileges:true",
        // Root filesystem is read-only. Writes go to the checkout or to tmpfs.
        "--read-only",
        "--tmpfs",
        TMP_TMPFS,
        "--tmpfs",
        HOME_TMPFS,
        // Bounded resources, so one worker cannot starve the host.
        "--pids-limit",
        "512",
        "--memory",
        "4g",
        "--memory-swap",
        "4g",
        "--cpus",
        "2",
        "--ulimit",
        "nofile=4096:8192",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

/// Arguments for a worker container. The environment is passed from a file, not on the
/// command line.
pub(crate) fn worker_run_args(
    network: &str,
    checkout_root: &str,
    env_file: &str,
    task_id: &str,
    image: &str,
    command: &[String],
) -> Vec<String> {
    let mut args = vec![
        "run".to_string(),
        "-d".to_string(),
        "--rm".to_string(),
        "--workdir".to_string(),
        "/workspace".to_string(),
        "--volume".to_string(),
        format!("{checkout_root}:/workspace"),
        "--network".to_string(),
        network.to_string(),
        "--label".to_string(),
        format!("frontal-code.task_id={task_id}"),
        "--label".to_string(),
        format!("{TASK_NETWORK_CONTAINER_LABEL}={network}"),
        "--env-file".to_string(),
        env_file.to_string(),
    ];
    args.extend(worker_hardening_args());
    args.push(image.to_string());
    args.extend(command.iter().cloned());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_network_names_are_safe_docker_names() {
        assert_eq!(
            task_network_name("task_6af0597f_1"),
            "frontal-task-task_6af0597f_1"
        );
        let escaped = task_network_name("a/../../b");
        assert!(escaped.starts_with("frontal-task-a") && escaped.ends_with('b'));
        assert!(!escaped.contains('/') && !escaped.contains('.'));
        assert!(task_network_name(&"x".repeat(200)).len() <= "frontal-task-".len() + 48);
    }

    #[test]
    fn worker_container_is_hardened_and_sees_no_host_socket() {
        let args = worker_run_args(
            "frontal-task-t1",
            "/tmp/checkout",
            "/tmp/worker.env",
            "t1",
            "frontal-code-worker:local",
            &["code".to_string(), "hosted".to_string()],
        );
        let joined = args.join(" ");
        assert!(joined.contains("--cap-drop ALL"));
        assert!(joined.contains("--security-opt no-new-privileges:true"));
        assert!(joined.contains("--read-only"));
        assert!(joined.contains("--network frontal-task-t1"));
        assert!(joined.contains("--env-file /tmp/worker.env"));
        assert!(joined.contains("--memory 4g"));
        assert!(joined.contains("--pids-limit 512"));
        // Never privileged, never the Docker socket, never the host network.
        assert!(!args.iter().any(|arg| arg == "--privileged"));
        assert!(!joined.contains("docker.sock"));
        assert!(!args.iter().any(|arg| arg == "host"));
        // The image and command come last.
        assert_eq!(
            args[args.len() - 2..],
            ["code".to_string(), "hosted".to_string()]
        );
    }

    #[test]
    fn no_credentials_appear_on_the_command_line() {
        let args = worker_run_args("n", "/c", "/e", "t", "img", &[]);
        assert!(!args
            .iter()
            .any(|arg| arg.contains("_API_KEY") || arg.contains("_TOKEN")));
    }

    #[test]
    fn default_relay_image_is_pinned_by_digest() {
        assert!(crate::DEFAULT_RELAY_IMAGE.contains("@sha256:"));
        assert!(!crate::DEFAULT_RELAY_IMAGE.ends_with(":latest"));
    }

    #[test]
    fn relay_forwards_one_port_and_is_hardened() {
        let args = relay_run_args("alpine/socat", 8788);
        assert!(args.contains(&"TCP-LISTEN:8788,fork,reuseaddr".to_string()));
        assert!(args.contains(&"TCP4:host.docker.internal:8788".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["--cap-drop", "ALL"]));
        assert!(args.contains(&"--read-only".to_string()));
    }

    #[test]
    fn each_server_port_gets_its_own_relay() {
        // Two servers on one Docker host must not replace each other's relay.
        assert_eq!(relay_container_name(8788), "frontal-code-relay-8788");
        assert_ne!(relay_container_name(8788), relay_container_name(8789));
        let args = relay_run_args("alpine/socat", 8789);
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--name", "frontal-code-relay-8789"]));
    }
}
