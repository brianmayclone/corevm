use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use anyhow::{bail, Context, Result};

pub fn generate_self_signed(target: &Path, cn: &str) -> Result<(PathBuf, PathBuf)> {
    let tls_dir = target.join("etc/vmm/tls");
    fs::create_dir_all(&tls_dir).context("Failed to create TLS directory")?;

    let cert_path = tls_dir.join("server.crt");
    let key_path = tls_dir.join("server.key");

    let subject = format!("/CN={}", cn);
    let output = Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "rsa:4096",
            "-sha256",
            "-days",
            "3650",
            "-nodes",
            "-keyout",
            key_path.to_str().context("Invalid key path")?,
            "-out",
            cert_path.to_str().context("Invalid cert path")?,
            "-subj",
            &subject,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .context("Failed to execute openssl")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("openssl failed for CN={}: {}", cn, stderr);
    }

    Ok((cert_path, key_path))
}

pub fn import_certificates(target: &Path, cert_src: &Path, key_src: &Path) -> Result<()> {
    let tls_dir = target.join("etc/vmm/tls");
    fs::create_dir_all(&tls_dir).context("Failed to create TLS directory")?;

    let cert_dst = tls_dir.join("server.crt");
    let key_dst = tls_dir.join("server.key");

    fs::copy(cert_src, &cert_dst)
        .with_context(|| format!("Failed to copy certificate from {:?}", cert_src))?;
    fs::copy(key_src, &key_dst)
        .with_context(|| format!("Failed to copy key from {:?}", key_src))?;

    Ok(())
}
