use frontal_code_sandbox::{
    build_macos_sandbox_command, FilesystemIsolationMode, SandboxRequest, SandboxStatus,
};
#[allow(unused_imports)]
use std::path::Path;

#[test]
fn profile_with_off_filesystem_mode() {
    #[cfg(target_os = "macos")]
    {
        let status = SandboxStatus {
            enabled: true,
            requested: SandboxRequest {
                enabled: true,
                namespace_restrictions: true,
                network_isolation: false,
                filesystem_mode: FilesystemIsolationMode::Off,
                allowed_mounts: vec![],
            },
            supported: true,
            active: true,
            namespace_supported: true,
            namespace_active: true,
            network_supported: true,
            network_active: false,
            filesystem_mode: FilesystemIsolationMode::Off,
            filesystem_active: false,
            allowed_mounts: vec![],
            in_container: false,
            container_markers: vec![],
            fallback_reason: None,
        };
        let result = build_macos_sandbox_command("echo test", Path::new("/workspace"), &status);
        if let Some(cmd) = result {
            let profile = &cmd.args[1];
            assert!(profile.contains("(allow default)"));
            assert!(!profile.contains("(deny file-write*)"));
        }
    }
}

#[test]
fn profile_with_workspace_only_and_allowlist_mounts() {
    #[cfg(target_os = "macos")]
    {
        let status = SandboxStatus {
            enabled: true,
            requested: SandboxRequest {
                enabled: true,
                namespace_restrictions: true,
                network_isolation: false,
                filesystem_mode: FilesystemIsolationMode::WorkspaceOnly,
                allowed_mounts: vec!["/extra/mount".to_string()],
            },
            supported: true,
            active: true,
            namespace_supported: true,
            namespace_active: true,
            network_supported: true,
            network_active: false,
            filesystem_mode: FilesystemIsolationMode::WorkspaceOnly,
            filesystem_active: true,
            allowed_mounts: vec!["/extra/mount".to_string()],
            in_container: false,
            container_markers: vec![],
            fallback_reason: None,
        };
        let result = build_macos_sandbox_command("echo test", Path::new("/workspace"), &status);
        if let Some(cmd) = result {
            let profile = &cmd.args[1];
            assert!(profile.contains("(deny file-write*)"));
            assert!(profile.contains("(allow file-write* (subpath \"/workspace\"))"));
            assert!(profile.contains("(allow file-write* (subpath \"/extra/mount\"))"));
        }
    }
}

#[test]
fn profile_with_allow_list_mode() {
    #[cfg(target_os = "macos")]
    {
        let status = SandboxStatus {
            enabled: true,
            requested: SandboxRequest {
                enabled: true,
                namespace_restrictions: true,
                network_isolation: false,
                filesystem_mode: FilesystemIsolationMode::AllowList,
                allowed_mounts: vec!["/data".to_string(), "/logs".to_string()],
            },
            supported: true,
            active: true,
            namespace_supported: true,
            namespace_active: true,
            network_supported: true,
            network_active: false,
            filesystem_mode: FilesystemIsolationMode::AllowList,
            filesystem_active: true,
            allowed_mounts: vec!["/data".to_string(), "/logs".to_string()],
            in_container: false,
            container_markers: vec![],
            fallback_reason: None,
        };
        let result = build_macos_sandbox_command("echo test", Path::new("/workspace"), &status);
        if let Some(cmd) = result {
            let profile = &cmd.args[1];
            assert!(profile.contains("(deny file-write*)"));
            assert!(profile.contains("(allow file-write* (subpath \"/data\"))"));
            assert!(profile.contains("(allow file-write* (subpath \"/logs\"))"));
            assert!(!profile.contains("/workspace"));
        }
    }
}

#[test]
fn profile_with_network_isolation() {
    #[cfg(target_os = "macos")]
    {
        let status = SandboxStatus {
            enabled: true,
            requested: SandboxRequest {
                enabled: true,
                namespace_restrictions: true,
                network_isolation: true,
                filesystem_mode: FilesystemIsolationMode::Off,
                allowed_mounts: vec![],
            },
            supported: true,
            active: true,
            namespace_supported: true,
            namespace_active: true,
            network_supported: true,
            network_active: true,
            filesystem_mode: FilesystemIsolationMode::Off,
            filesystem_active: false,
            allowed_mounts: vec![],
            in_container: false,
            container_markers: vec![],
            fallback_reason: None,
        };
        let result = build_macos_sandbox_command("echo test", Path::new("/workspace"), &status);
        if let Some(cmd) = result {
            let profile = &cmd.args[1];
            assert!(profile.contains("(deny network*)"));
            assert!(!profile.contains("(deny file-write*)"));
        }
    }
}

#[test]
fn profile_with_both_network_and_filesystem_isolation() {
    #[cfg(target_os = "macos")]
    {
        let status = SandboxStatus {
            enabled: true,
            requested: SandboxRequest {
                enabled: true,
                namespace_restrictions: true,
                network_isolation: true,
                filesystem_mode: FilesystemIsolationMode::WorkspaceOnly,
                allowed_mounts: vec![],
            },
            supported: true,
            active: true,
            namespace_supported: true,
            namespace_active: true,
            network_supported: true,
            network_active: true,
            filesystem_mode: FilesystemIsolationMode::WorkspaceOnly,
            filesystem_active: true,
            allowed_mounts: vec![],
            in_container: false,
            container_markers: vec![],
            fallback_reason: None,
        };
        let result = build_macos_sandbox_command("echo test", Path::new("/workspace"), &status);
        if let Some(cmd) = result {
            let profile = &cmd.args[1];
            assert!(profile.contains("(deny network*)"));
            assert!(profile.contains("(deny file-write*)"));
            assert!(profile.contains("(allow file-write* (subpath \"/workspace\"))"));
        }
    }
}

#[test]
fn profile_starts_with_version_and_allow_default() {
    #[cfg(target_os = "macos")]
    {
        let status = SandboxStatus {
            enabled: true,
            requested: SandboxRequest {
                enabled: true,
                namespace_restrictions: true,
                network_isolation: false,
                filesystem_mode: FilesystemIsolationMode::WorkspaceOnly,
                allowed_mounts: vec![],
            },
            supported: true,
            active: true,
            namespace_supported: true,
            namespace_active: true,
            network_supported: true,
            network_active: false,
            filesystem_mode: FilesystemIsolationMode::WorkspaceOnly,
            filesystem_active: true,
            allowed_mounts: vec![],
            in_container: false,
            container_markers: vec![],
            fallback_reason: None,
        };
        let result = build_macos_sandbox_command("echo test", Path::new("/workspace"), &status);
        if let Some(cmd) = result {
            let profile = &cmd.args[1];
            assert!(profile.starts_with("(version 1) (allow default)"));
        }
    }
}

#[test]
fn profile_handles_paths_with_special_characters() {
    #[cfg(target_os = "macos")]
    {
        let status = SandboxStatus {
            enabled: true,
            requested: SandboxRequest {
                enabled: true,
                namespace_restrictions: true,
                network_isolation: false,
                filesystem_mode: FilesystemIsolationMode::WorkspaceOnly,
                allowed_mounts: vec!["/path/with\"quotes".to_string()],
            },
            supported: true,
            active: true,
            namespace_supported: true,
            namespace_active: true,
            network_supported: true,
            network_active: false,
            filesystem_mode: FilesystemIsolationMode::WorkspaceOnly,
            filesystem_active: true,
            allowed_mounts: vec!["/path/with\"quotes".to_string()],
            in_container: false,
            container_markers: vec![],
            fallback_reason: None,
        };
        let result = build_macos_sandbox_command("echo test", Path::new("/workspace"), &status);
        if let Some(cmd) = result {
            let profile = &cmd.args[1];
            assert!(profile.contains("with\\\"quotes"));
        }
    }
}

#[test]
fn profile_with_no_writable_mounts_in_workspace_only() {
    #[cfg(target_os = "macos")]
    {
        let status = SandboxStatus {
            enabled: true,
            requested: SandboxRequest {
                enabled: true,
                namespace_restrictions: true,
                network_isolation: false,
                filesystem_mode: FilesystemIsolationMode::WorkspaceOnly,
                allowed_mounts: vec![],
            },
            supported: true,
            active: true,
            namespace_supported: true,
            namespace_active: true,
            network_supported: true,
            network_active: false,
            filesystem_mode: FilesystemIsolationMode::WorkspaceOnly,
            filesystem_active: true,
            allowed_mounts: vec![],
            in_container: false,
            container_markers: vec![],
            fallback_reason: None,
        };
        let result = build_macos_sandbox_command("echo test", Path::new("/workspace"), &status);
        if let Some(cmd) = result {
            let profile = &cmd.args[1];
            assert!(profile.contains("(allow file-write* (subpath \"/workspace\"))"));
        }
    }
}

#[test]
fn profile_with_allow_list_and_no_mounts_grants_no_writes() {
    #[cfg(target_os = "macos")]
    {
        let status = SandboxStatus {
            enabled: true,
            requested: SandboxRequest {
                enabled: true,
                namespace_restrictions: true,
                network_isolation: false,
                filesystem_mode: FilesystemIsolationMode::AllowList,
                allowed_mounts: vec![],
            },
            supported: true,
            active: true,
            namespace_supported: true,
            namespace_active: true,
            network_supported: true,
            network_active: false,
            filesystem_mode: FilesystemIsolationMode::AllowList,
            filesystem_active: true,
            allowed_mounts: vec![],
            in_container: false,
            container_markers: vec![],
            fallback_reason: None,
        };
        let result = build_macos_sandbox_command("echo test", Path::new("/workspace"), &status);
        if let Some(cmd) = result {
            let profile = &cmd.args[1];
            assert!(profile.contains("(deny file-write*)"));
            let allow_count = profile.matches("(allow file-write*").count();
            assert_eq!(allow_count, 0);
        }
    }
}
